# Changelog

What changed in each SketchCraft release, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Scale figures.** New models start with a person standing at the origin,
  a flat figure that always turns to face you, for a sense of size (as in
  SketchUp). Pick who it is under **Document › Scale figure**: a person, the
  SketchCraft beaver with a clipboard, or nobody. On April 1, the beaver
  clocks in. Both are in the Library, renders turn them to the camera too,
  and any component can be set to **Always face the camera**.
- **Move a single corner.** With nothing selected, the Move tool grabs the
  corner under the cursor: its edges stretch as you go, and the faces around
  it fold to stay flat (as in SketchUp).
- **Windows portable ZIP.** `SketchCraft_<version>_x64_portable.zip`: unzip
  anywhere and open SketchCraft.exe, no installer. When a new version is out,
  the portable copy links to the new ZIP instead of installing.
- **Classic layout.** The tool bar, view bar and workspace strip dock edge to
  edge above the model, outside it, like SketchUp, and the Value box sits in
  the corner. Shift+S › **Classic layout**, or per workspace under Customize ›
  Toolbar. The floating layout stays the default.
- **Plugins.** Small JavaScript plugins add commands (File › Plugins ▸, and
  Shift+S). They run sandboxed: no network, no files, only engine commands,
  and what they change can be undone. Drop a folder in
  ~/Documents/SketchCraft/Plugins; see [docs/PLUGINS.md](docs/PLUGINS.md).
- **Cut list** (the first plugin). Lists the parts to cut, grouped by size and
  material, and roughly how many 4×8 sheets and 8 ft boards to buy. Copy the
  table or save it as CSV.
- **Icons in skins.** A skin can bring its own tool icons: in the skin editor,
  **Load icons…** takes image files named after the tools (line.svg,
  push.png…; **File names** lists them). They're saved in the .sketchskin
  file, so an icon set can be shared like any skin.

## [0.3.4] - 2026-11-15

### Changed
- **Menus fit the window.** The File menu is shorter: every export is under
  one **Export ▸** entry. All menus scroll when the window is short, and the
  right-click menu opens upward or leftward near an edge instead of running
  off the screen.
- **Fields on brand.** Every dropdown, text box and number box shares one look
  from the skin: its border and corners, a brand chevron instead of the
  browser's arrow, a violet focus ring, and violet checkboxes, sliders and
  color wells. Dark skins follow along.
- **The side panel fits.** Dropdowns in the right panel (Style's Preset, Faces
  and Background) stay inside it and line up, instead of running off the edge.

### Added
- **Axis gizmo.** The red, green and blue axes sit in the model's top-right
  corner and turn with the view. Drag it to orbit; click an axis to look along
  it (Z for the top view, the hollow −Y for the front), and click the axis
  facing you again to look from the other side.
- **See your version.** The bottom of the File menu shows which SketchCraft
  you're running, and **About SketchCraft** in Shift+S says it too.

## [0.3.3] - 2026-11-15

### Added
- **Automatic updates.** The desktop app checks for a new version a few
  seconds after it opens (and every six hours), and offers **Update and
  restart**. Updates are signed and verified before they install. **Help ›
  Check for updates…** (in the File menu and Shift+S) checks right away. Where a
  copy can't update itself, it links to the download page.

### Changed
- **The tool bar fits the window.** Tools that don't fit go behind a
  **+N more** button instead of running off the edges. Its list is searchable:
  type a tool's name (or shortcut) and press Enter. **+ Add or remove tools…**
  at the bottom customizes the bar.

## [0.3.2] - 2026-11-15

### Added
- **Center and Origin snapping.** Drawing, Move and Tape Measure now snap to the
  center of any arc or circle (a green ring, "Center") and to the model's origin
  (a yellow square, "Origin"), so you can move an object from a circle's center
  or put it back at the origin. Thanks to Common User on the ArtCraft Discord
  for the report.
- **Render settings.** The Render dialog's new **More settings** holds the
  output size (HD, 4K, square, portrait or custom), exposure, sunlight, sky
  light, sun color (cool to golden hour), light bounces, a plain background
  color instead of the sky, the ground on or off and its color, and denoise.
  They're remembered between renders. The `photoreal` command takes the same
  options (`sky_strength`, `background`, `ground`, `ground_color`, `warmth`).

### Fixed
- **Windows: DWG and IFC helpers are found.** SketchCraft now finds
  `dwg2dxf.exe` and the ODA File Converter in its versioned
  `Program Files\ODA` folder, and finds Python for IFC as `python.exe`, `py` or
  `%LOCALAPPDATA%\SketchCraft\ifc`. Opening an IFC file without IfcOpenShell
  shows Windows setup steps.
- The README banner and brand images are in the repository (an ignore rule had
  kept them out).

## [0.3.1] - 2026-11-15

### Fixed
- **macOS:** the whole app is signed (ad hoc, with the hardened runtime), so a
  downloaded copy no longer looks "damaged". macOS still asks the first time,
  since it isn't notarized yet: **System Settings › Privacy & Security › Open
  Anyway**.
- The release workflow signs and notarizes with a Developer ID once the Apple
  secrets are set (see [docs/SIGNING.md](docs/SIGNING.md)).

## [0.3.0] - 2026-11-15

Open the files architects send you, and draw like SketchUp.

### Added
- **Files**
  - AutoCAD DWG, open and save (with LibreDWG or the free ODA File Converter
    installed). DXF brings in text, dimensions, splines, ellipses and hatches.
  - IFC, so Revit and ArchiCAD models open (with IfcOpenShell installed). Each
    element becomes a group, typed and tagged by storey.
  - Point clouds: PLY, LAS, XYZ and PTS scans, in color. Drawing tools snap to
    scan points.
- **Floor plans to 3D:** right-click an imported floor plan and choose
  **Raise walls** to pull its walls up, with window openings (sill and head).
  Imports land on the drawing, not the sheet border around it.
- **Drawing and selecting**
  - Arc from its center, 3-Point Arc, Pie, Rotated Rectangle and Freehand.
  - Double-click a face for it and its edges; triple-click for everything
    connected.
  - Doors and windows cut their opening in the wall they're placed on, through
    thick walls.
  - The Axes tool. Tape Measure rescales the model.
  - Shift+S searches every command. The outliner has search. Hide Rest of
    Model, Paste in Place.
- **Scenes and looks**
  - Scenes remember their style, tags, hidden things and section cut. Named
    section planes.
  - Physically based finishes (Matte, Satin, Gloss, Metal, Glass, Glow) in
    renders, glTF and a new view style.
  - Dynamic Components: position, rotation and Component Options.
  - Bundled Bricolage Grotesque and IBM Plex Mono.

### Changed
- IFC export validated with IfcOpenShell.

### Security
- The dev bridge refuses other websites and DNS rebinding; a strict content
  security policy; importers fuzzed (no crashes in 20,000-file runs);
  dependencies updated (`cargo audit`: no vulnerabilities). See
  [docs/SECURITY.md](docs/SECURITY.md).

## [0.2.0] - 2026-11-13

The first build with installers (never published as a release).

### Added
- The engine: welded vertices, edges and faces with holes, push/pull, face
  splitting, groups, components, tags, guides, soft edges and saved views, all
  driven by JSON commands with undo and redo.
- The desktop app (Tauri and React): SketchUp-style tools and shortcuts, an
  outliner, entity info, a measurement box, dimensions and text, and templates.
- An MCP server (`sketchcraft mcp`) and CLI that edit the same live model as
  the app.
- An asset library (doors, windows, trees, cars, streetlights and more), OBJ
  import and export, and PNG renders with dimensions and labels.
- Installers for macOS, Windows and Linux, and the CLI for each.

[Unreleased]: https://github.com/jasonsutter87/sketchcraft/compare/v0.3.4...HEAD
[0.3.4]: https://github.com/jasonsutter87/sketchcraft/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/jasonsutter87/sketchcraft/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/jasonsutter87/sketchcraft/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/jasonsutter87/sketchcraft/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/jasonsutter87/sketchcraft/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jasonsutter87/sketchcraft/releases/tag/v0.2.0
