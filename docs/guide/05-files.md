# 5. Files

## Your models

**File › Save** writes a `.sketchcraft` file. SketchCraft also keeps a live
copy as you work, so a crash doesn't lose your model.

## Restoring an earlier version

SketchCraft keeps earlier copies of your model in a `Backups` folder beside it:
one every few minutes while you work (older ones are thinned to about one an hour,
then one a day), and an extra one just before anything that removes a lot: New,
Open, a big delete, or a plugin run that wiped much of the model.

**File › Restore from backup…** (or **Shift+S**, type "restore") lists them, newest
first, with the time, the size and what the copy was taken before ("before New",
"before a big shrink", "before a plugin run"). For each one:

- **Restore** puts that version in place as one step. **Undo (⌘Z) takes the restore
  back**, and the model it replaced is kept as a backup first, so you can't lose
  anything by trying.
- **Save a copy** leaves your model alone and puts that version in a `Restored`
  folder beside it, so you can open it with **File › Open…** and compare.

The same list shows work SketchCraft **set aside** for you: a `.unsaved-` file
(your changes, kept when another program saved over the model while a plugin was
running or your own save was failing) and a `.damaged-` file (a copy of a model
file that couldn't be read). Restoring from those works the same way. SketchCraft
never deletes these without telling you.

Backups made because an AI agent or a plugin did something big are kept in their
own small share (at most ten each), so they can't push out your own.

If two programs edit the same model (for example Claude over MCP), **Undo** takes
back only *your* last step, never the other program's work. If the other program has
since changed or built on the same objects, Undo says so and skips that step instead
of guessing.

## Models that are too big

A model can hold about two million points, edges and faces. A bigger one (an
enormous import) still opens, for looking at and for deleting parts of it; it can't
be saved over your file until it is under the limit, and the file on disk isn't touched
meanwhile.

## Opening other formats

**File › Open…** or **Import…** (⌘I):

| Format | From | Notes |
| --- | --- | --- |
| `.skp` | SketchUp 6 to 8 and 2013 onward | geometry, groups, components, colors and textures |
| `.dwg`, `.dxf` | AutoCAD and most CAD apps | DXF works on its own. DWG needs a free helper (below) |
| `.ifc` | Revit, ArchiCAD and other BIM apps | needs a free helper (below). Each element becomes a group, tagged by storey |
| `.fbx`, `.obj` | Blender, game and 3D apps | |
| `.ply`, `.las`, `.xyz`, `.pts` | 3D scans | point clouds in color; drawing tools snap to scan points |

### The DWG and IFC helpers

- **DWG:** install [LibreDWG](https://www.gnu.org/software/libredwg/) (on a
  Mac: `brew install libredwg`) or the free ODA File Converter. SketchCraft
  finds either one.
- **IFC:** SketchCraft uses [IfcOpenShell](https://ifcopenshell.org). Open an
  IFC file without it and SketchCraft shows the exact commands to set it up.

## From a floor plan to walls

Import a 2D floor plan (DWG or DXF), right-click it and choose **Raise walls**:
the walls go up, with openings where the windows are.

## Exporting

**File › Export ▸**: glTF (`.glb`), OBJ, STL (for 3D printing), Collada
(`.dae`), 3D DXF, DWG, IFC, a PDF or PNG of the view, a scene tour (MP4), and
section cuts (SVG, DXF). Also on the File menu: **Make blueprint (PDF)**,
**Drawing sheets (PDF)** at true scale, and **Generate report (CSV)**.

Next: [Presenting](06-presenting.md)
