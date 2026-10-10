# pdfcraft-takeoff

Layer L4 adds quantity take-off on top of `pdfcraft-measure`, for counting and totalling items on construction drawings.

Count markers are small filled circle (Circle) annotations centred on the symbol being counted, such as a gully, manhole or lighting column. Each records the item it counts in `/PCTakeoffItem` and its number within that item in `/PCTakeoffNumber`; its comment text reads "Gully #3". New markers are numbered one past the highest number already used for that item anywhere in the document. Markers are ordinary annotations, so moving, restyling and deleting them uses the existing comment workflow, and adding one is undoable.

Saved measurements belong to the item their label names. `summarise` totals counts (unit `nr`), lengths (distances and perimeters together) and areas per item and unit. Item names match case-insensitively and ignore surrounding spaces; blank labels are grouped as `Untagged`. One item measured in different units stays on separate lines rather than being converted. `csv` writes the summary as spreadsheet-safe CSV.

`apply_look` adds take-off styling to a saved measurement: dashed lines and a caption showing the value, the item and value, or nothing, at a chosen text size (`/PCMeasureCaptionSize`). Count markers take a colour and size.

In the desktop app a floating toolbar above the page holds Distance, Perimeter, Area, Count, Drawing scale and Take-off summary. Picking a tool switches the left panel to that tool's options: the item (with quick picks of items already in the document), line or marker style, label style, snapping and the current scale. Automation provides `takeoff_count`, `takeoff_counts`, `takeoff_summary` and `takeoff_export`.

Area measurements can carry a depth in `/PCTakeoffDepth [start end]`; `volumes` multiplies each area by its average depth, and the summary totals them per item. `viewport` lists, replaces and removes a page's drawing scales and converts between common units (mm, cm, m, km, in, ft, yd).

Not yet supported: numbers drawn on the markers themselves.
