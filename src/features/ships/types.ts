/** A selectable catalog ship (`list_ships`), with the user's favourite flag. */
export interface ShipEntry {
  /** Stable id: the game's class name, e.g. `CRUS_Starlifter_C2`. */
  id: string;
  name: string;
  manufacturer: string;
  manufacturerCode: string;
  /** Whole SCU. */
  cargoScu: number;
  maxContainerScu: number | null;
  isFavorite: boolean;
  /** Added by the user: manufacturer "Custom", id `custom:…`. */
  isCustom: boolean;
}

/** `create_custom_ship` / `update_custom_ship` input. */
export interface CustomShipInput {
  name: string;
  /** Whole SCU, 1–100,000. */
  cargoScu: number;
}

/** `get_selected_ship` / `set_selected_ship`. `stale`: the saved ship left the list and was cleared. */
export interface SelectedShip {
  ship: ShipEntry | null;
  stale: boolean;
}

/** `get_ship_catalog_info`. */
export interface CatalogInfo {
  count: number;
  gameVersion: string;
  generatedAt: string;
  source: string;
}
