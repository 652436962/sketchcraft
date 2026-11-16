# 5. Files

## Your models

**File › Save** writes a `.sketchcraft` file. SketchCraft also keeps a live
copy as you work, so a crash doesn't lose your model.

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
