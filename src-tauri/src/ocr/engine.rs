//! Runs the bundled `tesseract.exe` as a child process.
//!
//! - The image goes through a temporary PNM file in the app's own OCR temp folder; the guard
//!   deletes it on every path (success, error, timeout, cancellation, panic).
//! - TSV comes back on stdout; stdout and stderr are drained on threads so a large output can
//!   never fill the pipe and block the child.
//! - `CREATE_NO_WINDOW`: no console flashes.
//! - Each call has a timeout and watches a cancellation flag; either one kills the child.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::image::Image;
use super::OcrError;

pub const EXE_NAME: &str = "tesseract.exe";
pub const LANGUAGE: &str = "eng";
const POLL: Duration = Duration::from_millis(10);

/// Cancels running recognitions (set when the form closes).
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    /// Whether both handles control the same recognition.
    pub fn same_as(&self, other: &CancelToken) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Where the engine lives and where it may write temporary files.
#[derive(Debug, Clone)]
pub struct Engine {
    pub exe: PathBuf,
    pub tessdata: PathBuf,
    pub temp_dir: PathBuf,
    pub timeout: Duration,
}

impl Engine {
    /// The bundle layout: `<dir>/tesseract.exe` and `<dir>/tessdata/eng.traineddata`.
    pub fn in_dir(dir: &Path, temp_dir: PathBuf, timeout: Duration) -> Self {
        let dir = plain_path(dir);
        Self {
            exe: dir.join(EXE_NAME),
            tessdata: dir.join("tessdata"),
            temp_dir: plain_path(&temp_dir),
            timeout,
        }
    }

    /// `engineMissing` unless the executable and the English data are both present.
    pub fn check_files(&self) -> Result<(), OcrError> {
        let data = self.tessdata.join(format!("{LANGUAGE}.traineddata"));
        for path in [&self.exe, &data] {
            if !path.is_file() {
                return Err(OcrError::EngineMissing(format!(
                    "{} not found",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    /// Runs `--version` and returns its first line, e.g. `tesseract 5.5.3`.
    pub fn version(&self) -> Result<String, OcrError> {
        self.check_files()?;
        let mut cmd = Command::new(&self.exe);
        cmd.arg("--version");
        let out = run_child(cmd, self.timeout, &CancelToken::default())?;
        let text = if out.stdout.trim().is_empty() {
            out.stderr
        } else {
            out.stdout
        };
        text.lines()
            .next()
            .map(|l| l.trim().to_string())
            .filter(|l| l.starts_with("tesseract"))
            .ok_or_else(|| {
                OcrError::EngineMissing(format!("{} did not report a version", self.exe.display()))
            })
    }

    /// Recognises `image` with page segmentation mode `psm`; returns the TSV text.
    pub fn recognize_tsv(
        &self,
        image: &Image,
        psm: u8,
        cancel: &CancelToken,
    ) -> Result<String, OcrError> {
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        let input = TempFile::create(&self.temp_dir, image)?;
        let mut cmd = Command::new(&self.exe);
        cmd.arg(input.path())
            .arg("stdout")
            .arg("--tessdata-dir")
            .arg(&self.tessdata)
            .args([
                "-l",
                LANGUAGE,
                "--psm",
                &psm.to_string(),
                "-c",
                "tessedit_create_tsv=1",
            ]);
        let out = run_child(cmd, self.timeout, cancel)?;
        if !out.success {
            return Err(OcrError::Failed(format!(
                "tesseract exited with an error: {}",
                out.stderr.lines().next().unwrap_or("").trim()
            )));
        }
        Ok(out.stdout)
    }
}

/// Tauri returns verbatim paths (`\\?\C:\...`). Tesseract builds file names inside such a folder
/// with `/`, which verbatim paths do not accept, so plain drive paths drop the prefix.
/// UNC (`\\?\UNC\...`) and other paths are left unchanged.
pub fn plain_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/* ------------------------------------------------------------- process */

pub struct ChildOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

fn drain(mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Kills the child if it is still running when dropped (timeout, cancel, panic).
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Spawns `cmd` without a console, waits at most `timeout`, and kills it on timeout or cancel.
pub fn run_child(
    mut cmd: Command,
    timeout: Duration,
    cancel: &CancelToken,
) -> Result<ChildOutput, OcrError> {
    no_window(&mut cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = Running(cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => OcrError::EngineMissing(e.to_string()),
        _ => OcrError::Failed(format!("could not start the OCR engine: {e}")),
    })?);
    let stdout = drain(child.0.stdout.take().expect("piped stdout"));
    let stderr = drain(child.0.stderr.take().expect("piped stderr"));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child
            .0
            .try_wait()
            .map_err(|e| OcrError::Failed(e.to_string()))?
        {
            break status;
        }
        if cancel.is_cancelled() {
            drop(child); // kills and reaps
            return Err(OcrError::Cancelled);
        }
        if started.elapsed() >= timeout {
            drop(child);
            return Err(OcrError::Timeout);
        }
        std::thread::sleep(POLL);
    };
    Ok(ChildOutput {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/* ---------------------------------------------------------- temp files */

/// A uniquely named PNM file in the OCR temp folder, deleted when dropped.
pub struct TempFile(PathBuf);

impl TempFile {
    pub fn create(dir: &Path, image: &Image) -> Result<Self, OcrError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(dir)
            .map_err(|e| OcrError::Failed(format!("OCR temp folder: {e}")))?;
        let ext = if image.channels == 1 { "pgm" } else { "ppm" };
        let path = dir.join(format!(
            "ocr-{}-{}.{ext}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| OcrError::Failed(format!("OCR temp file: {e}")))?;
        let guard = TempFile(path); // from here on, the file is removed on every path
        image
            .write_pnm(std::io::BufWriter::new(file))
            .map_err(|e| OcrError::Failed(format!("OCR temp file: {e}")))?;
        Ok(guard)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Removes leftovers from a crashed run (`ocr-*.p?m`); called before the first recognition.
pub fn clear_temp_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("ocr-") && (name.ends_with(".pgm") || name.ends_with(".ppm")) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
