import { Boxes, MapPin, PackageCheck } from "lucide-react";
import { CollapsiblePanel, EmptyState, PageContainer, PageHeader, Panel, PanelHeader, Tag } from "../components/ui";

/**
 * Cargo overview shell.
 *
 * Layout rule: the two panels sit side by side whenever the *usable content
 * width* is at least 990 CSS px, and stack only below that. This is driven by
 * a CSS container query on the page content (not a viewport media query and
 * not orientation), so a tall/portrait window that is still >= 990 px wide
 * keeps the two-column layout.
 */
export function CargoPage() {
  return (
    <PageContainer wide>
      <div className="@container">
        <PageHeader
          index="02"
          title="Cargo"
          description="Pickup and destination summary with an informational cargo arrangement."
          actions={<span className="label-tech">0 SCU active</span>}
        />

        <div
          data-testid="cargo-layout"
          className="grid grid-cols-1 gap-4 @min-[990px]:grid-cols-[minmax(320px,2fr)_minmax(0,3fr)] @min-[990px]:items-start"
        >
          <div className="flex min-w-0 flex-col gap-4">
            <CollapsiblePanel title="Pickup" meta={<span className="label-tech">0 SCU</span>}>
              <EmptyState icon={MapPin} title="No pickups" compact>
                Pickup locations and their commodities will be listed here.
              </EmptyState>
            </CollapsiblePanel>
            <CollapsiblePanel title="Destination" meta={<span className="label-tech">0 SCU</span>}>
              <EmptyState icon={PackageCheck} title="No deliveries" compact>
                Destination locations and their commodities will be listed here.
              </EmptyState>
            </CollapsiblePanel>
          </div>

          <Panel>
            <PanelHeader title="Cargo arrangement" meta={<Tag>Not available yet</Tag>} />
            <div className="relative px-4 py-6">
              <DeckOutline />
              <div className="mt-2">
                <EmptyState icon={Boxes} title="Nothing to visualise" compact>
                  A read-only isometric view of active cargo will appear here. Select a ship later to scale the
                  view; capacity is informational only.
                </EmptyState>
              </div>
            </div>
          </Panel>
        </div>
      </div>
    </PageContainer>
  );
}

/** Decorative empty isometric deck grid (no cargo data). */
function DeckOutline() {
  const cols = 8;
  const rows = 5;
  const w = 36; // half-width of one cell in px (isometric 2:1)
  const h = w / 2;
  const ox = rows * w;
  const lines: string[] = [];
  for (let c = 0; c <= cols; c++) {
    lines.push(`M${ox + c * w},${c * h} L${ox + c * w - rows * w},${c * h + rows * h}`);
  }
  for (let r = 0; r <= rows; r++) {
    lines.push(`M${ox - r * w},${r * h} L${ox - r * w + cols * w},${r * h + cols * h}`);
  }
  const vw = (cols + rows) * w;
  const vh = (cols + rows) * h;
  return (
    <svg
      viewBox={`-2 -2 ${vw + 4} ${vh + 4}`}
      className="mx-auto block h-auto w-full max-w-xl text-line-strong"
      aria-hidden="true"
    >
      <path d={lines.join(" ")} fill="none" stroke="currentColor" strokeWidth="1" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
