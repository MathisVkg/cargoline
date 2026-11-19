import { CircleCheck, CircleX, MoreHorizontal, Pencil } from "lucide-react";
import { ActionMenu } from "./ActionMenu";
import type { FinalAction } from "./finalize";

/** Request to finish one whole contract (opens the confirmation dialog). */
export type RequestFinalize = (contractId: number, action: FinalAction) => void;

/** Request to edit one contract (opens the contract form in edit mode). */
export type RequestEdit = (contractId: number) => void;

/** Tick (`done`) or untick the reminder of these stops (one row can combine several). */
export type ToggleReminder = (stopIds: number[], done: boolean) => void;

interface ContractActionsMenuProps {
  contractId: number;
  contractRef: string;
  title: string;
  onRequest: RequestFinalize;
  onEdit: RequestEdit;
  /** "icon": a ⋯ button (By Contract heading); "chip": the C01 reference chip (By Location rows). */
  variant: "icon" | "chip";
}

/** Per-contract menu: Edit / Mark complete / Abandon. Always acts on the whole contract. */
export function ContractActionsMenu({ contractId, contractRef, title, onRequest, onEdit, variant }: ContractActionsMenuProps) {
  const name = `${contractRef} · ${title}`;
  return (
    <ActionMenu
      label={`Actions for contract ${name}`}
      title={variant === "chip" ? `${name} — contract actions` : undefined}
      heading={
        <>
          <p className="truncate text-sm font-medium text-fg">{name}</p>
          <p className="text-[11px] text-dim">Applies to the whole contract and all its cargo</p>
        </>
      }
      items={[
        { id: "edit", label: "Edit contract", icon: Pencil, onSelect: () => onEdit(contractId) },
        { id: "complete", label: "Mark complete", icon: CircleCheck, onSelect: () => onRequest(contractId, "complete") },
        { id: "abandon", label: "Abandon contract", icon: CircleX, danger: true, onSelect: () => onRequest(contractId, "abandon") },
      ]}
      triggerClassName={
        variant === "icon"
          ? "grid size-8 shrink-0 place-items-center rounded-sm text-dim transition-colors duration-150 hover:bg-raised hover:text-fg aria-expanded:bg-raised aria-expanded:text-fg"
          : "w-fit rounded-sm border border-transparent px-1 font-mono text-muted transition-colors duration-150 hover:border-line-strong hover:text-fg aria-expanded:border-line-strong aria-expanded:text-fg"
      }
    >
      {variant === "icon" ? <MoreHorizontal className="size-4" aria-hidden="true" /> : contractRef}
    </ActionMenu>
  );
}
