/** Sent on `window` when a citation style is added or removed, so style
 * pickers list it. */
export const CITATION_STYLES_CHANGED = "paperloom:citation-styles";

/** The repository's citation formats, as the settings name them. */
export const STYLE_FORMATS: Record<string, string> = {
  numeric: "Numeric",
  "author-date": "Author-date",
  note: "Footnotes",
};
