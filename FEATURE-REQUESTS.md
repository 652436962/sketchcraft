# Feature requests

What people have asked for, where it came from, and where it stands. The
current sprint is worked top to bottom. Android is tracked separately as
**SketchCraft Lite** (its own app).

Status: `todo` · `in progress` · `done` · `later` · `won't do`

## Sprint 1 (from 2026-11-16)

| # | Feature | Asked by | Added | Status | Completed |
| - | ------- | -------- | ----- | ------ | --------- |
| 1 | **Scale figure at the origin**: an original 2D person that always faces the camera, in new models and in the library, for a sense of size | EuphoricPenguin22 (Reddit) | 2026-11-16 | done | 2026-11-16 |
| 1b | **The beaver as the scale figure**: the mascot with a clipboard, as an option (Document › Scale figure), and on April 1 for everyone | Jason | 2026-11-16 | done | 2026-11-16 |
| 2 | **Select a vertex and move it**: pick a single point and drag it; the faces around it follow | Just_A_Random_Passer (Reddit) | 2026-11-16 | done | 2026-11-16 |
| 3 | **Windows portable ZIP**: the desktop app as a ZIP, no installer | mcam (Discord) | 2026-11-16 | done (first build in the next release) | 2026-11-16 |
| 4 | **Classic layout**: toolbars docked to the edges like SketchUp, no floating margins, more room for the model; the current layout stays | MaiJames (Reddit) | 2026-11-16 | done | 2026-11-16 |
| 5 | **Plugins**: small scripts that add commands and panels, using the same commands as the CLI and MCP | rats4final (Reddit) | 2026-11-16 | done | 2026-11-16 |
| 6 | **Cut list plugin**: plan plywood and lumber cuts from the model (the first plugin) | SoCaFroal (Reddit) | 2026-11-16 | done | 2026-11-16 |
| 7 | **Icons in skins**: a skin can swap the tool icons (a SketchUp-style icon set) | Fenrir (Discord) | 2026-11-16 | done | 2026-11-16 |
| 4b | **SketchUp-style look as the default**: a Classic skin with the docked layout for new installs; Workshop and the rest move to the gallery | Jason | 2026-11-16 | done | 2026-11-16 |
| 8 | **A clearer Skins button**: now **Themes**, one window for theme, layout and icons | Jason | 2026-11-16 | done | 2026-11-16 |

## Sprint 2: plugins, then SketchUp's Ruby (from 2026-11-16)

SketchCraft's own plugins grow up first; the Ruby layer then reuses the same
buttons, forms and tools.

| # | Feature | Asked by | Added | Status | Completed |
| - | ------- | -------- | ----- | ------ | --------- |
| 1 | **Plugin manager**: one window listing every plugin (name, version, author, built in or yours), turn each on or off, open its folder | rats4final (Reddit) | 2026-11-16 | done | 2026-11-16 |
| 2 | **Install a plugin from a file**: pick a .zip; it's checked and unpacked into the Plugins folder | rats4final (Reddit) | 2026-11-16 | done | 2026-11-16 |
| 3 | **Input forms**: `ctx.ask()` shows a form (numbers with units, text, choices, checkboxes) and returns the answers | Jason | 2026-11-16 | done | 2026-11-16 |
| 4 | **Toolbar buttons**: a plugin command can have an icon and sit on the tool bar | Jason | 2026-11-16 | done | 2026-11-16 |
| 5 | **Interactive tools**: a plugin can add a tool that gets clicks in the model (snapped points, the face under the cursor) and draws previews | Jason | 2026-11-16 | done | 2026-11-16 |
| 6 | **Example plugins**: stairs generator (form), framing (tool), to show and test the API | Jason | 2026-11-16 | done (Shelves, Stud wall) | 2026-11-16 |
| 7 | **SketchUp Ruby, a subset**: run .rb scripts and install .rbz files using the core of SketchUp's Ruby API (model, entities, add_face, add_line, pushpull, groups, materials, menus, input boxes) | rats4final (Reddit) | 2026-11-16 | done | 2026-11-16 |
| 8 | **Test with real free SketchUp scripts**, and document what works and what doesn't | Jason | 2026-11-16 | done | 2026-11-16 |

## Sprint 3: 0.3.7, drafting (from 2026-11-17)

LayOut-class documentation. People who draft for a living say the money is here, and SketchUp's LayOut does it badly.
Tracked on GitHub as the **0.3.7 — Drafting** milestone.

| # | Feature | Asked by | Added | Status | Completed |
| - | ------- | -------- | ----- | ------ | --------- |
| 1 | **Line weight and line style per tag and per edge**, printed at true weight, no stacked viewports | hardluxe (Reddit) | 2026-11-17 | planned | |
| 2 | **.pat hatches per tag**, with hatch colour, thickness, scale and angle | hardluxe (Reddit) | 2026-11-17 | planned | |
| 3 | **Cut fill per tag** in sections (poché) | hardluxe (Reddit) | 2026-11-17 | planned | |
| 4 | **Opacity per viewport** on sheets, for hybrid drawings without fog | hardluxe (Reddit) | 2026-11-17 | planned | |
| 5 | **Tables and text blocks** on sheets (schedules, drawing index) | Relative-Fondant6544 (Reddit) | 2026-11-17 | planned | |
| 6 | **Large documents stay fast**: A1, 50+ pages, lots of text, with a performance test | Relative-Fondant6544 (Reddit) | 2026-11-17 | planned | |
| 7 | **Interactive sheet editor**: place and size viewports, text, dimensions, tables, title blocks by mouse | Jason | 2026-11-17 | planned | |
| 8 | **AI drafting**: "dimension this plan", "hatch by tag", "lay out a permit set" through the agents | hardluxe (Reddit) | 2026-11-17 | planned | |

## Sprint 4: 0.3.8, texturing (from 2026-11-17)

Built-in UV and material tools for rendering and game assets, so the half-dozen texture add-ons aren't needed.
Tracked on GitHub as the **0.3.8 — Texturing** milestone.

| # | Feature | Asked by | Added | Status | Completed |
| - | ------- | -------- | ----- | ------ | --------- |
| 1 | **Position Texture**: move, rotate, scale and skew a texture on a face with pins | Relative-Fondant6544 (Reddit) | 2026-11-17 | planned | |
| 2 | **Projection mapping**: planar, box, cylinder, sphere | Relative-Fondant6544 (Reddit) | 2026-11-17 | planned | |
| 3 | **Paint-through**: continuous textures across connected faces | Relative-Fondant6544 (Reddit) | 2026-11-17 | planned | |
| 4 | **UV unwrap** with a UV view | Relative-Fondant6544 (Reddit) | 2026-11-17 | planned | |
| 5 | **Vertex painting** | Relative-Fondant6544 (Reddit) | 2026-11-17 | planned | |
| 6 | **glTF / GLB export** with UVs and vertex colours, for game engines | Relative-Fondant6544 (Reddit) | 2026-11-17 | planned | |
| 7 | **Textures in SketchUp 2017–2020 imports** | timrowledge (GitHub #1) | 2026-11-17 | planned | |

## Later

| Feature | Asked by | Added | Status | Notes |
| ------- | -------- | ----- | ------ | ----- |
| SketchCraft Lite (Android) | RuliAvo25, Lasm998 (Reddit) | 2026-11-16 | later | Its own app; touch-first push/pull |
| More of SketchUp's Ruby API: mouse tools (Sketchup::Tool), HtmlDialog, observers, followme, textures | rats4final (Reddit) | 2026-11-16 | later | The geometry-building core runs (sprint 2, #7) |
| Library of scale figures (people, cyclists, cars) for pathways | EuphoricPenguin22 (Reddit) | 2026-11-16 | later | Original art, traced from CC0 photos |
| 2D drafting done right: a dimension style editor (arrows, text, units, precision) and fonts for text and tables, what people miss in CADCraft | Waste-Affect5021 (r/ArtCraftAI, about CADCraft) | 2026-11-16 | in 0.3.7 | Part of sprint 3 (drafting) |

## Done before this list

| Feature | Asked by | Completed |
| ------- | -------- | --------- |
| Snap to arc and circle centers, and the origin | Common User (Discord) | 2026-11-15 (0.3.2) |
| More render settings | Common User (Discord) | 2026-11-15 (0.3.2) |
| Import/export grouped in a submenu | Safe-Release-2251 (Reddit) | 2026-11-15 (0.3.4) |
| CLI and MCP | saltyudders (Reddit) | 0.1.0 |
| Themes (skins) | Fenrir (Discord) | 0.2.0 |
