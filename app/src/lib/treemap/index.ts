// Treemap de Desván: renderizador WebGL2 independiente del framework.
export { TreemapView, type TreemapHit, type TreemapLabel, type TreemapOptions, type TreemapTheme } from "./TreemapView";
export {
  STRIDE,
  FIELD,
  RFLAG,
  CAT_DIR,
  LAYOUT_HEADER_BYTES,
  decodeLayout,
  encodeLayout,
  rectInfo,
  isFrame,
  type LayoutMeta,
  type DecodedLayout,
  type RectInfo,
} from "./buffer";
export { CATEGORIES, CATEGORY_COLORS, CATEGORY_LABELS, CATEGORY_IDS, THEME, type CategoryInfo } from "./palette";
export { RectGrid } from "./grid";
export { ellipsize, planLabels, LabelOccupancy } from "./labels";
