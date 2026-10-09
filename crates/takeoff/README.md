# pdfcraft-takeoff

Layer L4 adds quantity take-off on top of `pdfcraft-measure`, for counting and totalling items on construction drawings.

Count markers are small filled circle (Circle) annotations centred on the symbol being counted, such as a gully, manhole or lighting column. Each records the item it counts in `/PCTakeoffItem` and its number within that item in `/PCTakeoffNumber`; its comment text reads "Gully #3". New markers are numbered one past the highest number already used for that item anywhere in the document. Markers are ordinary annotations, so moving, restyling and deleting them uses the existing comment workflow, and adding one is undoable.

Saved measurements belong to the item their label names. `summarise` totals counts (unit `nr`), lengths (distances and perimeters together) and areas per item and unit. Item names match case-insensitively and ignore surrounding spaces; blank labels are grouped as `Untagged`. One item measured in different units stays on separate lines rather than being converted. `csv` writes the summary as spreadsheet-safe CSV.

The desktop Measure panel has a Count tool (its Label field names the item) and a take-off table with CSV export. Automation provides `takeoff_count`, `takeoff_counts`, `takeoff_summary` and `takeoff_export`.

Not yet supported: numbers drawn on the markers themselves, volumes, and conversion between units.
