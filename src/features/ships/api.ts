import { call } from "../contracts/api";
import type { CatalogInfo, CustomShipInput, SelectedShip, ShipEntry } from "./types";

export function listShips(): Promise<ShipEntry[]> {
  return call<ShipEntry[]>("list_ships");
}

export function getSelectedShip(): Promise<SelectedShip> {
  return call<SelectedShip>("get_selected_ship");
}

/** `null` = No ship. Informational only: contracts and runs are never touched. */
export function setSelectedShip(id: string | null): Promise<SelectedShip> {
  return call<SelectedShip>("set_selected_ship", { id });
}

/** Stars or unstars a ship; never changes the selection. */
export function setShipFavorite(id: string, favorite: boolean): Promise<ShipEntry> {
  return call<ShipEntry>("set_ship_favorite", { id, favorite });
}

/** Adds a custom ship. Validation errors: `validation` with paths `name` / `cargoScu`. */
export function createCustomShip(input: CustomShipInput): Promise<ShipEntry> {
  return call<ShipEntry>("create_custom_ship", { input });
}

export function updateCustomShip(id: string, input: CustomShipInput): Promise<ShipEntry> {
  return call<ShipEntry>("update_custom_ship", { id, input });
}

/** Deletes a custom ship (and its favourite / the selection). Returns whether it was selected. */
export function deleteCustomShip(id: string): Promise<boolean> {
  return call<boolean>("delete_custom_ship", { id });
}

export function getShipCatalogInfo(): Promise<CatalogInfo> {
  return call<CatalogInfo>("get_ship_catalog_info");
}
