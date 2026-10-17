# Notes page covers

Status: implemented; real desktop and Android visual acceptance remains pending. A curated image collection is planned.

## Designs and color

Add cover opens a picker with **Designs** and **Upload**, reusing the Add icon picker's search, shuffle, color control, and upload panel. Designs appear in two groups:

| Group | Designs |
| --- | --- |
| Simple | Solid, Gradient, Contours, Mosaic, Dots, Grid |
| Illustrations | Study, Finance, Nature, Studio, Mathematics, Programming, Atlas, Observatory |

Illustrations depict recognizable subjects (learning, finance, the outdoors, creative work, mathematics, software, geography, science) as connected scenes rather than scattered icons. Notation and code inside them are decorative and language-independent.

A designed cover stores its design and either Automatic (the theme foreground) or one of the 32 theme palette slots shared with icons and Calendar. Colors resolve against the active theme, so changing themes updates covers, gallery cards, and history previews. Uploaded images keep their original colors. Default color and Ask every time preferences are remembered separately from icons.

Designs render as static CSS and small inline SVG, without generated image files, raster thumbnails, animation, blur filters, or resize listeners, to keep covers cheap to display. Pattern density, angle, animation, secondary colors, and arbitrary color controls are out of scope.

## Selection and editing

Without a cover, Add cover appears above the title. An existing cover shows Change and, for images, Reposition, on hover or focus on desktop and always on touch; mobile also offers them from Note actions. Choosing a design or uploading an image saves immediately and closes the picker; there is no confirmation step. Dismissing the picker never reverts applied choices, and a late file selection cannot update a different page. Failed saves keep the picker open with an error for retry.

Upload supports the desktop file picker, mobile file input, and paste, with PNG, JPG, and WebP validation and byte and dimension limits. Uploads are managed assets with the normal ownership and cleanup lifecycle.

Reposition lets the user drag the image within the banner by mouse, pen, or touch, or move it with arrow keys. Moves update a local preview only; Save position persists one normalized focal point, and Cancel, Escape, Android Back, or leaving the page discards the draft. Original image bytes are never cropped or resampled.

## Geometry

Banners follow the page container width, including previews and tablet layouts. Height is one fifth of the width, bounded between 8rem and 15rem and capped at a quarter of the viewport height, which gives roughly 5:1 banners on wide layouts and deeper crops on narrow ones. Each renderer centers the saved focal point where possible and clamps to the image edges. There is no required upload aspect ratio; wide sources give more crop flexibility. Gallery cards keep their fit preference: focal points control filled previews, while fitted previews show the whole image. History previews use the same geometry. Titles stay below the banner.

## Persistence and transfer

Designed covers store a validated design identifier and palette choice; image covers store a managed asset reference and an optional normalized focal point. No user-supplied CSS or SVG is stored. Frontend and Rust boundaries validate design identifiers, palette bounds, finite focal coordinates, and asset references. Duplication, templates, history, and graph export preserve cover descriptors, while rendering always uses the viewer's current theme.

External image URL covers are disabled on every platform by the `notes.external-image-references` capability, so the picker offers no URL tab. Imported external cover variants remain part of the import contract.

HTML and Markdown exports omit cover appearance and report `page_cover_omitted`, because the exporters have no theme-rendering contract. Rendering a static, theme-resolved cover into visual exports is planned.

## Planned curated collection

A planned **Collection** tab offers CC0 images chosen for flexible banner crops. Each entry needs a stable identifier, thumbnail, source dimensions, suggested focal point, description, and provenance and license information. Selected images must be stored locally so covers work offline and survive catalog changes, and opening Add cover must never load a remote catalog. A small bundled starter set is preferred; optional downloads are a separate decision. The tab and catalog infrastructure are added only when actual content exists.
