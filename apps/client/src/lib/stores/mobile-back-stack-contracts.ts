export interface MobileBackLayer {
  /** Close or step back within this layer. */
  handle: () => void;
}

export interface MobileBackStack {
  readonly hasActiveLayer: boolean;
  activate: (layer: MobileBackLayer) => () => void;
  consume: () => boolean;
}
