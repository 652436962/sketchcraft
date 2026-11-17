//! The command layer. Everything that changes or inspects a model goes
//! through `Engine::exec`, whether it comes from a mouse in the app, a script,
//! the CLI or an AI agent over MCP. Commands are JSON:
//!
//!   {"cmd":"rect","from":[0,0,0],"to":[8,12,0]}
//!   {"cmd":"pushpull","face":7,"distance":6}
//!
//! Each reply is JSON too: {"ok":true, ...} or {"ok":false,"error":"..."}.

use crate::export;
use crate::history::Step;
use crate::model::{Id, Model};
use crate::policy::Caller;
use crate::render::{self, RenderOpts};
use anyhow::{bail, Result};
use glam::DVec3;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

// `Engine::run`'s arms live in one file per area. Each works on the model being edited
// (inside `edit`, the open component's definition), borrowed field by field so an arm can
// still reach the engine's other fields; hence a macro, not a method.
macro_rules! active_model {
    ($engine:expr) => {
        match $engine.ctx {
            Some(d) => std::sync::Arc::make_mut(&mut $engine.model.definitions.get_mut(&d).unwrap().model),
            None => &mut $engine.model,
        }
    };
}

mod annotate;
mod draw;
mod files;
mod look;
mod session;
mod structure;

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Start an empty model.
    /// A blank model; `figure` places a scale figure ("person" or "beaver") at the origin.
    New {
        #[serde(default)]
        figure: Option<String>,
    },
    /// Axis-aligned rectangle between two opposite corners (one coordinate must match).
    Rect { from: [f64; 3], to: [f64; 3] },
    /// Planar polygon through the points (a building footprint, a gable end...).
    Polygon { points: Vec<[f64; 3]> },
    /// Connected lines; if the last point closes the loop, a face is made.
    Line { points: Vec<[f64; 3]> },
    /// Circle (or regular polygon with few `segments`) as a face.
    Circle {
        center: [f64; 3],
        radius: f64,
        #[serde(default = "up")]
        normal: [f64; 3],
        #[serde(default = "circle_segments")]
        segments: usize,
    },
    /// Arc as edges, from `start` to `end`, bowing out by `bulge` meters.
    Arc {
        start: [f64; 3],
        end: [f64; 3],
        bulge: f64,
        #[serde(default = "up")]
        normal: [f64; 3],
        #[serde(default = "arc_segments")]
        segments: usize,
    },
    /// Offset a face's outline: + inward (new face inside), - outward (ring).
    Offset { face: Id, distance: f64 },
    /// Sweep a profile face along a path: `points`, `edges`, or `around` a face's outline.
    FollowMe {
        face: Id,
        #[serde(default)]
        points: Option<Vec<[f64; 3]>>,
        #[serde(default)]
        edges: Option<Vec<Id>>,
        #[serde(default)]
        around: Option<Id>,
    },
    /// Pull a face out (positive) or push it in (negative) along its normal.
    /// `toward`: pull that way whichever way the face happens to look
    /// (a new rectangle on the ground can face up or down).
    #[serde(alias = "push_pull")]
    Pushpull {
        face: Id,
        distance: f64,
        #[serde(default)]
        toward: Option<[f64; 3]>,
    },
    /// Move faces, edges, vertices or whole groups.
    Move { ids: Vec<Id>, by: [f64; 3] },
    /// Rotate around `center` about `axis` (default: vertical), in degrees.
    Rotate {
        ids: Vec<Id>,
        center: [f64; 3],
        #[serde(default = "up")]
        axis: [f64; 3],
        angle: f64,
    },
    /// Scale from `center`: one number for uniform, or [x, y, z].
    Scale { ids: Vec<Id>, center: [f64; 3], factor: Factor },
    /// Copy faces, groups or components, offset by `by`.
    Duplicate { ids: Vec<Id>, by: [f64; 3] },
    /// `count` more copies spaced `by` apart (a row of houses, a fence).
    Array { ids: Vec<Id>, by: [f64; 3], count: usize },
    Delete { ids: Vec<Id> },
    /// Flip faces (or every face in a group).
    Reverse { ids: Vec<Id> },
    /// Color faces (or every face in a group), 0..1 RGB.
    Paint {
        ids: Vec<Id>,
        #[serde(default)]
        color: Option<[f32; 3]>,
        /// a material by name (an image material from `texture`)
        #[serde(default)]
        material: Option<String>,
    },
    /// Create or update an image material. `data` is a data: URL; `width` and
    /// `height` are meters per tile; `project` [x,y,z] projects it along a
    /// direction (null turns projection off); `color` is its average color.
    Texture {
        name: String,
        #[serde(default)]
        data: Option<String>,
        #[serde(default)]
        width: Option<f64>,
        #[serde(default)]
        height: Option<f64>,
        #[serde(default)]
        color: Option<[f32; 3]>,
        #[serde(default)]
        project: Option<Option<[f64; 3]>>,
        #[serde(default)]
        origin: Option<[f64; 3]>,
    },
    /// New geometry goes into a fresh group until end_group.
    BeginGroup { #[serde(default)] name: Option<String> },
    EndGroup,
    /// Wrap existing faces into a group.
    Group { ids: Vec<Id>, #[serde(default)] name: Option<String> },
    /// List faces, optionally only one group's, or only those facing a direction.
    Faces {
        #[serde(default)]
        group: Option<Id>,
        #[serde(default)]
        facing: Option<[f64; 3]>,
    },
    /// The face under a point (on its plane, inside its outline), or the
    /// nearest face center if none contains it. Optional facing filter.
    Pick {
        at: [f64; 3],
        #[serde(default)]
        facing: Option<[f64; 3]>,
    },
    /// Duplicate a group (a whole house), offset by `by`.
    Copy { group: Id, by: [f64; 3], #[serde(default)] name: Option<String> },
    /// Counts, bounds, groups, materials.
    Info,
    /// Each part's true size (along its own faces, so a turned board reads
    /// right): `ids` (groups or components), or every leaf group and component.
    Parts {
        #[serde(default)]
        ids: Vec<Id>,
    },
    /// The plugins: built in, and those in ~/Documents/SketchCraft/Plugins.
    Plugins,
    /// Turn a plugin on or off (remembered in the Plugins folder).
    PluginEnable { id: String, on: bool },
    /// Install a plugin from a .zip: a file `path`, or its bytes as base64 `data`.
    InstallPlugin {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        data: Option<String>,
        /// the file's name, when sending `data` (a .rb script is told apart by it)
        #[serde(default)]
        name: Option<String>,
        /// only describe the plugin (what it is, its files, what it would replace); install nothing
        #[serde(default)]
        preview: bool,
        /// allow replacing a plugin that's already installed (the person has been asked)
        #[serde(default)]
        replace: bool,
    },
    /// Remove a plugin you installed (moved aside to Plugins/.removed, not deleted).
    RemovePlugin { id: String },
    /// Write text to a file (the app's Save CSV; plugins can't reach it).
    SaveText { path: String, text: String },
    /// Make faces or a group into a reusable component (placed where it was).
    MakeComponent { ids: Vec<Id>, #[serde(default)] name: Option<String> },
    /// Place a component (by id or name) at a point, optionally turned about Z.
    Place {
        component: Value,
        at: [f64; 3],
        #[serde(default)]
        rotate: f64,
        #[serde(default)]
        name: Option<String>,
        /// the face it's placed on: a component that cuts openings cuts one there
        #[serde(default)]
        host: Option<Id>,
    },
    /// Whether a component cuts an opening in the face it's placed on.
    ComponentCuts { id: Id, cuts: bool },
    /// Whether a component always turns to face the camera (flat figures).
    ComponentFaceMe { id: Id, on: bool },
    /// Open a group, or a component's definition through one of its placements; `close` to finish.
    Edit { id: Id },
    Close,
    /// Turn a component placement back into plain geometry.
    Explode { id: Id },
    /// List the component library.
    Components,
    Hide { ids: Vec<Id> },
    /// Unhide `ids`, or everything if none given.
    Unhide { #[serde(default)] ids: Vec<Id> },
    /// Put groups/components on a tag (layer); null tag removes it.
    Tag { ids: Vec<Id>, tag: Option<String> },
    TagVisible { tag: String, visible: bool },
    /// A tag's color (for Color by tag); null clears it.
    TagColor { tag: String, color: Option<[f32; 3]> },
    /// Rename a tag everywhere it's used.
    RenameTag { tag: String, to: String },
    /// Put groups and components inside a group (`parent`), or at the top (null).
    SetParent { ids: Vec<Id>, parent: Option<Id> },
    /// Dissolve a group: what's inside moves up a level.
    Ungroup { id: Id },
    /// Lock or unlock groups (locked: can't be moved, edited or deleted).
    Lock { ids: Vec<Id>, #[serde(default = "yes")] locked: bool },
    /// Soft edges aren't drawn. `ids` edges or faces; `coplanar:true` softens every flat seam.
    Soften {
        #[serde(default)]
        ids: Vec<Id>,
        #[serde(default = "yes")]
        soft: bool,
        #[serde(default)]
        coplanar: bool,
    },
    Rename { id: Id, name: String },
    /// Construction guide: a line through `point` along `direction`, or a point.
    Guide { point: [f64; 3], #[serde(default)] direction: Option<[f64; 3]> },
    ClearGuides,
    /// Display units: m, cm, mm, ft (feet-inches) or in. Geometry stays in meters.
    Units { units: String },
    /// A dimension from `from` to `to`, drawn `offset` away (e.g. [0,-1,0] = 1 m toward the street).
    Dimension { from: [f64; 3], to: [f64; 3], #[serde(default)] offset: [f64; 3] },
    /// A text label at `at`, with an optional leader line to `leader`.
    Text { at: [f64; 3], text: String, #[serde(default)] leader: Option<[f64; 3]> },
    /// Save a camera as a named view.
    /// `photo`: a picture seen from this camera (data:image URL), as Match Photo makes
    /// Tag visibility and what's hidden are remembered with it; `style` (the
    /// app's view style) and `section` ([origin, normal], or none) when given.
    SaveView {
        name: String,
        eye: [f64; 3],
        target: [f64; 3],
        #[serde(default = "fov")]
        fov: f64,
        #[serde(default)]
        photo: Option<String>,
        #[serde(default)]
        style: Option<serde_json::Value>,
        #[serde(default)]
        section: Option<[f64; 6]>,
        /// true: the camera only (SketchUp's scene with its other properties unticked)
        #[serde(default)]
        camera_only: bool,
    },
    /// Go to a saved view's state: tags shown / hidden and what's hidden as
    /// they were. Returns the view (camera, style, section) for the app.
    ShowView { name: String },
    DeleteView { name: String },
    Undo,
    Redo,
    /// Start one undo step that spans many commands (a plugin run); `end_batch` closes it.
    /// While it is open the model isn't saved after each command, and Undo is off.
    BeginBatch,
    /// `id`: the number `begin_batch` answered with; a stale one (its batch is already over) does nothing
    EndBatch {
        #[serde(default)]
        id: Option<u64>,
    },
    /// Render a PNG. Without `out`, just reports the camera it would use.
    Render {
        #[serde(default)]
        out: Option<String>,
        #[serde(flatten)]
        opts: RenderOpts,
    },
    Save { path: String },
    Open { path: String },
    /// Wavefront OBJ (+ .mtl) for Blender and friends.
    ExportObj { path: String },
    /// Binary glTF (.glb): Blender, three.js, game engines, Quick Look.
    ExportGlb { path: String },
    /// Binary STL for 3D printing; `scale` 1000 gives millimeters.
    ExportStl { path: String, #[serde(default = "one")] scale: f64 },
    /// Animation through saved views (or a turntable): `path` .mp4/.mov
    /// (needs ffmpeg) or a folder for PNG frames. Render options apply.
    Animate {
        path: String,
        #[serde(default = "anim_fps")]
        fps: f64,
        #[serde(default = "anim_transition")]
        transition: f64,
        #[serde(default = "one")]
        delay: f64,
        #[serde(default)]
        views: Vec<String>,
        #[serde(default = "anim_turntable")]
        turntable: f64,
        /// render the timeline instead of a tour of the scenes
        #[serde(default)]
        timeline: bool,
        #[serde(flatten)]
        opts: RenderOpts,
    },
    /// Drawing sheets as a PDF: pages of viewports (saved views or iso / top /
    /// front / ..., optionally at a scale like "1:100"), labels, scale bars
    /// and a title block. No pages: your saved views on one sheet.
    Sheets {
        path: String,
        #[serde(default = "sheet_a3")]
        sheet: String,
        #[serde(default)]
        project: Option<String>,
        #[serde(default)]
        pages: Vec<crate::layout::PageSpec>,
        /// white on blueprint blue
        #[serde(default)]
        blueprint: bool,
        /// lines in a box above the title block ("# " makes a heading)
        #[serde(default)]
        notes: Vec<String>,
    },
    /// A blueprint set in one go: every floor's plan and the four
    /// elevations, white on blue, at a true scale that fits the sheet.
    Blueprint {
        path: String,
        #[serde(default = "sheet_a3")]
        sheet: String,
        #[serde(default)]
        project: Option<String>,
        /// general notes under the area table
        #[serde(default)]
        notes: Vec<String>,
    },
    /// A door's swing for plans: the hinge, the strike point across the
    /// opening, and any point on the side it swings into.
    DoorSwing { hinge: [f64; 3], strike: [f64; 3], toward: [f64; 3] },
    /// The building's floor heights (m), found from its floors.
    Levels,
    /// Collada (.dae), Z up, meters: one object per group / component.
    ExportDae { path: String },
    /// 3D DXF: 3DFACE triangles in millimeters, a layer per object.
    ExportDxf { path: String },
    /// PDF: a rendered view (same options as render) on a landscape A4 sheet.
    ExportPdf {
        path: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(flatten)]
        opts: RenderOpts,
    },
    /// Import an OBJ file; each object becomes a group. `up` "y" (default) or "z".
    ImportObj {
        #[serde(default)]
        path: Option<String>,
        /// the file's contents, instead of a path (the app sends this)
        #[serde(default)]
        text: Option<String>,
        #[serde(default = "y_up")]
        up: String,
        #[serde(default = "one")]
        scale: f64,
        #[serde(default)]
        at: [f64; 3],
        /// component name (default: the file name)
        #[serde(default)]
        name: Option<String>,
    },
    /// Import a SketchUp file (SketchUp 6-8 era). `path`, or the file as base64 in `data`.
    ImportSkp {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        data: Option<String>,
        #[serde(default)]
        at: [f64; 3],
        #[serde(default)]
        name: Option<String>,
    },
    /// Solid tools on closed groups / components: union, subtract,
    /// intersect, trim, split, outer_shell. Subtract and trim: the first
    /// cuts the second.
    Solid { op: String, ids: Vec<Id> },
    /// Intersect Faces: draw lines where the picked faces / groups cross the
    /// model (`with: "model"`, default) or each other (`"selection"`).
    Intersect {
        ids: Vec<Id>,
        #[serde(default = "with_model")]
        with: String,
    },
    /// Flip Along an axis (red/x, green/y, blue/z) across the pick's middle.
    Flip { ids: Vec<Id>, axis: String },
    /// Section cut through `origin` facing `normal`: the cut lines and filled
    /// regions per object. With `path` (.svg or .dxf), writes it flat, in mm.
    Section {
        origin: [f64; 3],
        normal: [f64; 3],
        #[serde(default)]
        path: Option<String>,
    },
    /// Sandbox From Contours: contour edges (or groups of them) -> terrain.
    TerrainFromContours {
        ids: Vec<Id>,
        #[serde(default = "contour_spacing")]
        spacing: f64,
    },
    /// Sandbox From Scratch: a flat triangulated grid.
    TerrainGrid {
        #[serde(default)]
        at: [f64; 3],
        width: f64,
        depth: f64,
        #[serde(default = "one")]
        spacing: f64,
    },
    /// Sandbox Smoove: raise (or lower) a soft round area of a terrain.
    Smoove { group: Id, at: [f64; 3], radius: f64, height: f64 },
    /// Sandbox Stamp: flatten a pad under a footprint face, sloping out over `offset`.
    Stamp {
        group: Id,
        face: Id,
        #[serde(default = "one")]
        offset: f64,
    },
    /// Sandbox Drape: lay edges onto a terrain, splitting its faces.
    Drape { group: Id, ids: Vec<Id> },
    /// Sandbox Flip Edge: swap the diagonal between two triangles.
    FlipEdge { edge: Id },
    /// Match Photo: solve the camera from two red and two green lines drawn
    /// along a photo's edges (pixels, first line of each pair toward +axis)
    /// and the origin's spot; saves it as a view that keeps the photo.
    MatchPhoto {
        name: String,
        width: f64,
        height: f64,
        red: [[f64; 4]; 2],
        green: [[f64; 4]; 2],
        origin: [f64; 2],
        #[serde(default = "photo_distance")]
        distance: f64,
        #[serde(default)]
        photo: Option<String>,
    },
    /// Dynamic Component attributes on a placement (or defaults on a
    /// component, by its component id). An empty value removes one.
    SetAttrs { id: Id, attrs: std::collections::BTreeMap<String, String> },
    /// A point cloud (scan) from .ply, .las, .xyz, .pts or .txt. `units` the
    /// file's (default m); at most `max_points` (thinned evenly beyond that).
    ImportCloud {
        path: String,
        #[serde(default)]
        units: Option<String>,
        #[serde(default = "cloud_budget")]
        max_points: usize,
    },
    /// A cloud's points for showing: base64 little-endian f32 x, y, z and
    /// u8 r, g, b (empty when uncolored), at most `max` points.
    CloudData {
        id: Id,
        #[serde(default = "cloud_show")]
        max: usize,
    },
    /// Rename, hide / show, or delete a cloud.
    Cloud {
        id: Id,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        hidden: Option<bool>,
        #[serde(default)]
        delete: bool,
    },
    /// IFC (and so Revit's exports) through IfcOpenShell, if installed: one
    /// group per element, typed for the Classifier, grouped by storey.
    ImportIfc { path: String },
    /// Selection helpers: a face (or edge) and what it touches. `all`: everything
    /// connected to it (SketchUp's triple-click); else the face and its edges
    /// (double-click; an edge gives the faces on it). Read-only.
    Connected {
        id: Id,
        #[serde(default)]
        all: bool,
    },
    /// Every face with this material (null: unpainted) / every group and
    /// placement with this tag, in the open context. Read-only.
    SameAs {
        #[serde(default)]
        material: Option<String>,
        #[serde(default)]
        tag: Option<String>,
        #[serde(default)]
        by: String,
    },
    /// Resize the model (or the open group) by `factor` (Tape Measure: measure
    /// something, type its real length).
    Resize { factor: f64 },
    /// A named section plane through `origin`, keeping the side `normal`
    /// points to; it becomes the active cut. Named "Section N" when unnamed.
    SectionPlane {
        #[serde(default)]
        name: Option<String>,
        origin: [f64; 3],
        normal: [f64; 3],
    },
    /// Change a section plane: rename, move, make it the active cut, or delete.
    SectionEdit {
        id: Id,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        origin: Option<[f64; 3]>,
        #[serde(default)]
        normal: Option<[f64; 3]>,
        #[serde(default)]
        active: Option<bool>,
        #[serde(default)]
        delete: bool,
    },
    /// No section cut (the planes stay, ready to switch back on).
    SectionOff,
    /// Move the drawing axes (the Axes tool): origin, red direction, green
    /// direction (made square to red). `reset`: back to the model's own.
    SetAxes {
        #[serde(default)]
        origin: Option<[f64; 3]>,
        #[serde(default)]
        x: Option<[f64; 3]>,
        #[serde(default)]
        y: Option<[f64; 3]>,
        #[serde(default)]
        reset: bool,
    },
    /// A finish on faces (or everything in groups): each keeps its color or
    /// image, as a material "<its material> (<label>)" with these values.
    Finish {
        ids: Vec<Id>,
        label: String,
        #[serde(default)]
        roughness: Option<f32>,
        #[serde(default)]
        metalness: Option<f32>,
        #[serde(default)]
        glass: Option<bool>,
        #[serde(default)]
        emission: Option<f32>,
    },
    /// Component Options: the attributes a placement offers its user (those
    /// with an `_name_access` of textbox, list or view).
    Options { id: Id },
    /// Generate Report: every group and component with size, area, volume
    /// and attributes; `path` writes CSV.
    Report {
        #[serde(default)]
        path: Option<String>,
    },
    /// Classifier: mark groups / placements as an IFC type (IfcWall, IfcDoor,
    /// IfcWindow, IfcSlab, IfcRoof, IfcColumn, IfcBeam, IfcStair, ...);
    /// an empty type clears it.
    Classify { ids: Vec<Id>, class: String },
    /// IFC4 export: Project > Site > Building > Storey with every group and
    /// component as a typed, triangulated element.
    ExportIfc {
        path: String,
        #[serde(default)]
        project: Option<String>,
    },
    /// Add Location: terrain (and map imagery) of the area around lat / lon,
    /// `size` meters across, origin at ground level, +Y north.
    AddLocation {
        lat: f64,
        lon: f64,
        #[serde(default = "loc_size")]
        size: f64,
        #[serde(default = "loc_cells")]
        cells: usize,
        #[serde(default = "yes")]
        imagery: bool,
    },
    /// The component library: files you can place (default folder
    /// ~/Documents/SketchCraft/Library; subfolders are collections).
    Library {
        #[serde(default)]
        dir: Option<String>,
    },
    /// A thumbnail (PNG data URL) of a component in the model, a built-in
    /// asset, or a library file.
    Thumbnail {
        #[serde(default)]
        component: Option<Id>,
        #[serde(default)]
        asset: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default = "thumb_px")]
        size: u32,
    },
    /// Place a library file as a component (the same file again reuses it).
    InsertFile {
        path: String,
        at: [f64; 3],
        #[serde(default)]
        rotate: f64,
    },
    /// Save a component into the library (its parts come along).
    SaveToLibrary {
        component: Id,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        collection: Option<String>,
        #[serde(default)]
        dir: Option<String>,
    },
    /// Copy model files into the library.
    LibraryAdd {
        paths: Vec<String>,
        #[serde(default)]
        collection: Option<String>,
        #[serde(default)]
        dir: Option<String>,
    },
    /// Remove components nothing places.
    Purge,
    /// Take a material and its picture out of the model (the person's own command: an agent or a plugin can't).
    /// Faces that use it get `replace_with` (a material that exists), or the plain default look.
    DeleteMaterial {
        name: String,
        #[serde(default)]
        replace_with: Option<String>,
    },
    /// Remove every material no face uses (the person's own command).
    PurgeMaterials,
    /// Show a file or the library folder in Finder / Explorer.
    Reveal {
        #[serde(default)]
        path: Option<String>,
    },
    /// App skins in ~/Documents/SketchCraft/Skins (.sketchskin files, JSON).
    Skins,
    /// Save a skin there (`skin` is its JSON); returns the file's path.
    SaveSkin { name: String, skin: Value },
    /// Bevel (segments 1, a flat chamfer) or round (segments > 1) outside
    /// edges of solid groups by `size` meters.
    Bevel {
        edges: Vec<Id>,
        size: f64,
        #[serde(default = "one_seg")]
        segments: usize,
    },
    /// Split an edge into `count` equal parts.
    Divide { edge: Id, count: usize },
    /// Cut a solid group / component in two along a plane.
    Slice { id: Id, origin: [f64; 3], normal: [f64; 3] },
    /// A named material with a color (paint with it by name). Names say what
    /// it is to the photoreal renderer: glass / window, metal / steel /
    /// chrome, light / lamp; anything else is paint.
    /// `roughness` 0..1, `metalness` 0..1, `glass`, `emission`: how it looks
    /// in renders (and the viewport's physically based style).
    Material {
        name: String,
        #[serde(default)]
        color: Option<[f32; 3]>,
        #[serde(default)]
        roughness: Option<f32>,
        #[serde(default)]
        metalness: Option<f32>,
        #[serde(default)]
        glass: Option<bool>,
        #[serde(default)]
        emission: Option<f32>,
    },
    /// Photoreal render (path traced): sun from azimuth (degrees from north,
    /// clockwise) and altitude; `samples` per pixel (16 preview, 128+ final).
    Photoreal {
        #[serde(default)]
        out: Option<String>,
        #[serde(default = "pt_samples")]
        samples: u32,
        #[serde(default = "pt_bounces")]
        bounces: u32,
        /// sun direction; default: front-left of the view, for shadows
        #[serde(default)]
        sun_azimuth: Option<f64>,
        #[serde(default = "pt_altitude")]
        sun_altitude: f64,
        /// the real sun for the model's location (add_location; else 40°N
        /// 98°W): date "2026-06-21" and local time "15:30"
        #[serde(default)]
        date: Option<String>,
        #[serde(default)]
        time: Option<String>,
        /// hours the local clock is ahead of UTC (default: from the longitude)
        #[serde(default)]
        utc_offset: Option<f64>,
        /// add `samples` more to the same render each call (it sharpens);
        /// changing the camera, size, light or model starts over
        #[serde(default)]
        progressive: bool,
        #[serde(default = "pt_sun")]
        sun_strength: f32,
        #[serde(default = "yes")]
        denoise: bool,
        #[serde(default = "pt_exposure")]
        exposure: f32,
        /// how bright the sky's light is (1 = normal, 0 = sun only)
        #[serde(default = "pt_exposure")]
        sky_strength: f32,
        /// a plain backdrop instead of the sky, as "#rrggbb" (the sky still lights the model)
        #[serde(default)]
        background: Option<String>,
        /// the ground plane under the model (false: the model floats)
        #[serde(default = "yes")]
        ground: bool,
        /// the ground's color, "#rrggbb"
        #[serde(default)]
        ground_color: Option<String>,
        /// the sun's color: -1 cool .. 0 daylight .. 1 warm (golden hour)
        #[serde(default)]
        warmth: f32,
        #[serde(flatten)]
        opts: RenderOpts,
    },
    /// Model from several matched photos: corners marked in two or more
    /// photos are solved to 3D together with each photo's distance (see
    /// photomodel). Places guide points; `build` draws `edges` between them.
    PhotoSolve {
        points: Vec<crate::photomodel::PhotoPoint>,
        /// point index pairs to connect with lines (with `build`)
        #[serde(default)]
        edges: Vec<[usize; 2]>,
        /// keep this photo's distance (default: the first one marked)
        #[serde(default)]
        fixed: Option<String>,
        /// a known length: {a, b, length} (point indexes, meters)
        #[serde(default)]
        scale: Option<KnownLength>,
        #[serde(default = "yes")]
        place: bool,
        #[serde(default)]
        build: bool,
        /// make edges within a few degrees of an axis exactly square to it
        #[serde(default = "yes")]
        square: bool,
    },
    /// A smooth surface from a cage (Catmull-Clark, like Blender's
    /// Subdivision modifier): `points`, `faces` (index lists), optional
    /// per-face `materials`, sharp `creases` (index pairs), `levels` 0-5.
    Subd {
        #[serde(default)]
        name: Option<String>,
        points: Vec<[f64; 3]>,
        faces: Vec<Vec<usize>>,
        #[serde(default)]
        materials: Vec<Option<String>>,
        #[serde(default)]
        creases: Vec<[usize; 2]>,
        #[serde(default = "two")]
        levels: usize,
        /// keep the cage for SubD editing (cage_edit, the app's SubD mode)
        #[serde(default = "yes")]
        editable: bool,
        /// mirror plane for symmetric edits (found by itself when left out)
        #[serde(default)]
        symmetry: Option<crate::subd::Symmetry>,
    },
    /// Edit a subdivision object's cage, then rebuild its surface: move
    /// points ([[i, x, y, z], ...], mirrored when the cage is symmetric),
    /// crease/uncrease edges, and topology ops. One undo step.
    CageEdit {
        id: Id,
        #[serde(default)]
        moves: Vec<[f64; 4]>,
        #[serde(default = "yes")]
        mirror: bool,
        #[serde(default)]
        crease: Vec<[usize; 2]>,
        #[serde(default)]
        uncrease: Vec<[usize; 2]>,
        #[serde(default)]
        op: Option<CageOp>,
        #[serde(default)]
        levels: Option<usize>,
        /// "auto" (find the plane), "off", or an axis 0/1/2 (through the middle)
        #[serde(default)]
        symmetry: Option<Value>,
    },
    /// Smooth a group already modeled: it becomes its subdivision surface.
    /// `keep_sharp`: edges at a sharp angle stay creased.
    Subdivide {
        id: Id,
        #[serde(default = "two")]
        levels: usize,
        #[serde(default)]
        keep_sharp: bool,
    },
    /// Set the timeline: clips played in order (scenes, turntable, walk, sun).
    Timeline { clips: Vec<crate::timeline::Clip> },
    /// Every frame of the timeline: camera (and sun) per frame.
    TimelineFrames {
        #[serde(default = "anim_fps")]
        fps: f64,
    },
    /// The app's rendered frames for a video: one PNG/JPEG data URL each...
    VideoFrame { session: String, index: usize, data: String },
    /// ...then encode them (ffmpeg) to `path` (.mp4 / .mov) and clean up.
    VideoFinish { session: String, path: String, #[serde(default = "anim_fps")] fps: f64 },
    /// Import a DXF drawing (text DXF): a group with a group per layer, tagged
    /// by layer; closed outlines become faces. `units` overrides the file's.
    ImportDxf {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        units: Option<String>,
        #[serde(default)]
        at: [f64; 3],
        #[serde(default)]
        name: Option<String>,
    },
    /// Import an AutoCAD DWG (through LibreDWG or the ODA File Converter, if installed).
    ImportDwg {
        path: String,
        #[serde(default)]
        units: Option<String>,
        #[serde(default)]
        at: [f64; 3],
    },
    /// Walls from an imported plan: the thin faces between wall lines, pulled up
    /// `height` meters (in the group's wall layers if it has any).
    RaiseWalls {
        group: Id,
        #[serde(default = "wall_height")]
        height: f64,
        #[serde(default = "wall_thickness")]
        max_thickness: f64,
        /// windows found on the plan: wall below the sill and above the head
        #[serde(default = "window_sill")]
        sill: f64,
        #[serde(default = "window_head")]
        head: f64,
    },
    /// Export the model as an AutoCAD DWG (3D faces, through a converter).
    ExportDwg { path: String },
    /// Project a matched photo (a saved view with a photo) onto faces, or the
    /// faces of groups: each gets a straightened texture cut from the photo.
    ProjectPhoto {
        view: String,
        ids: Vec<Id>,
        #[serde(default = "photo_px")]
        size: u32,
    },
    /// Join connected edges into one curve (picked, offset and followed as one).
    Weld { edges: Vec<Id> },
    /// Split curves back into separate edges.
    ExplodeCurve { edges: Vec<Id> },
    /// Give one placement of a shared component its own definition.
    MakeUnique { id: Id },
    /// Flip a surface's faces to face the same way as `face`.
    OrientFaces { face: Id },
    /// 3D text from outlines (the app traces the font): `rings` in meters in the
    /// text plane, laid at `at` along `right` and `up`, pulled `depth` (0 = flat).
    #[serde(rename = "text_3d", alias = "text3d")]
    Text3d {
        rings: Vec<Vec<[f64; 2]>>,
        at: [f64; 3],
        #[serde(default = "x_axis")]
        right: [f64; 3],
        #[serde(default = "z_axis")]
        up: [f64; 3],
        #[serde(default)]
        depth: f64,
        #[serde(default)]
        name: Option<String>,
    },
    /// How see-through a material is in the view (reference images): 0-1.
    Opacity { material: String, value: f32 },
    /// Put the model in a ready-made setting: suburban, city, park, desert,
    /// waterfront, studio; or none to take it away.
    Environment { kind: String },
    /// Where the real sun is for the model's location at a local date and time.
    Sun {
        #[serde(default)]
        date: Option<String>,
        #[serde(default)]
        time: Option<String>,
        #[serde(default)]
        utc_offset: Option<f64>,
    },
    /// Write an image the app already has (a PNG data URL) to a file.
    SaveImage { path: String, data: String },
    /// Volume of a closed group / component (m³), or not a solid.
    Volume { id: Id },
    /// Import a binary FBX (7.x): `path`, or the file as base64 in `data`.
    /// The file becomes one component; each mesh a group inside it.
    ImportFbx {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        data: Option<String>,
        #[serde(default)]
        at: [f64; 3],
        #[serde(default)]
        name: Option<String>,
    },
    /// Walls along a path: `thickness` and `height`; `closed` makes a room.
    Wall {
        points: Vec<[f64; 3]>,
        #[serde(default = "wall_t")]
        thickness: f64,
        #[serde(default = "wall_h")]
        height: f64,
        #[serde(default)]
        closed: bool,
    },
    /// A straight stair climbing `rise` along `direction` (degrees from +X).
    Stairs {
        at: [f64; 3],
        rise: f64,
        #[serde(default)]
        direction: f64,
        #[serde(default = "stair_w")]
        width: f64,
        #[serde(default = "stair_t")]
        tread: f64,
    },
    /// A roof over a flat face: kind gable | hip | shed, pitch in degrees.
    Roof {
        face: Id,
        #[serde(default = "gable")]
        kind: String,
        #[serde(default = "pitch")]
        pitch: f64,
        #[serde(default = "overhang")]
        overhang: f64,
        #[serde(default = "roof_t")]
        thickness: f64,
    },
    /// A square (or `round`) column.
    Column {
        at: [f64; 3],
        #[serde(default = "col_size")]
        size: f64,
        #[serde(default = "wall_h")]
        height: f64,
        #[serde(default)]
        round: bool,
    },
    /// Place a built-in asset (door, window, garage_door, tree, palm, car, streetlight, bench, hydrant).
    Insert {
        asset: String,
        at: [f64; 3],
        #[serde(default)]
        rotate: f64,
        /// the face it's placed on (doors and windows cut their opening there)
        #[serde(default)]
        host: Option<Id>,
    },
    /// List the built-in assets.
    Assets,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Factor {
    Uniform(f64),
    Axes([f64; 3]),
}

fn pt_samples() -> u32 {
    32
}

fn pt_bounces() -> u32 {
    6
}

fn pt_altitude() -> f64 {
    32.0
}

fn pt_sun() -> f32 {
    2.4
}

fn pt_exposure() -> f32 {
    1.0
}

fn one_seg() -> usize {
    1
}

fn thumb_px() -> u32 {
    160
}

fn loc_size() -> f64 {
    300.0
}

fn loc_cells() -> usize {
    60
}

fn sheet_a3() -> String {
    "a3".into()
}

fn photo_distance() -> f64 {
    15.0
}

fn anim_fps() -> f64 {
    24.0
}

fn anim_transition() -> f64 {
    2.0
}

fn anim_turntable() -> f64 {
    8.0
}

fn contour_spacing() -> f64 {
    1.0
}

fn with_model() -> String {
    "model".into()
}

fn axis_index(a: &str) -> Result<usize> {
    Ok(match a.to_ascii_lowercase().as_str() {
        "red" | "x" => 0,
        "green" | "y" => 1,
        "blue" | "z" => 2,
        _ => bail!("axis is red, green or blue (x, y, z)"),
    })
}

fn circle_segments() -> usize {
    24
}

fn arc_segments() -> usize {
    12
}

fn wall_t() -> f64 {
    0.2
}
fn wall_h() -> f64 {
    2.7
}
fn stair_w() -> f64 {
    1.0
}
fn stair_t() -> f64 {
    0.28
}
fn gable() -> String {
    "gable".into()
}
fn pitch() -> f64 {
    30.0
}
fn overhang() -> f64 {
    0.4
}
fn roof_t() -> f64 {
    0.2
}
fn col_size() -> f64 {
    0.3
}

fn wall_height() -> f64 {
    2.7
}

fn wall_thickness() -> f64 {
    0.6
}

fn cloud_budget() -> usize {
    5_000_000
}

fn cloud_show() -> usize {
    2_000_000
}

fn window_sill() -> f64 {
    0.9
}

fn window_head() -> f64 {
    2.1
}

fn one() -> f64 {
    1.0
}

fn y_up() -> String {
    "y".into()
}

/// "#rrggbb" (or "rrggbb") to sRGB 0..1.
fn hex_rgb(s: &str) -> Result<[f32; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("a color is written #rrggbb, like #ffffff (got {s:?})");
    }
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map(|x| x as f32 / 255.0);
    Ok([v(0)?, v(2)?, v(4)?])
}

fn yes() -> bool {
    true
}

fn fov() -> f64 {
    45.0
}

fn two() -> usize {
    2
}

fn up() -> [f64; 3] {
    [0.0, 0.0, 1.0]
}

impl Command {
    pub fn mutates(&self) -> bool {
        !matches!(
            self,
            Command::Faces { .. }
                | Command::Volume { .. }
                | Command::Photoreal { .. }
                | Command::SaveImage { .. }
                | Command::Sun { .. }
                | Command::Library { .. }
                | Command::Thumbnail { .. }
                | Command::SaveToLibrary { .. }
                | Command::LibraryAdd { .. }
                | Command::Reveal { .. }
                | Command::Skins
                | Command::SaveSkin { .. }
                | Command::Report { .. }
                | Command::Options { .. }
                | Command::CloudData { .. }
                | Command::Connected { .. }
                | Command::SameAs { .. }
                | Command::Section { .. }
                | Command::Info
                | Command::Parts { .. }
                | Command::Plugins
                | Command::PluginEnable { .. }
                | Command::InstallPlugin { .. }
                | Command::RemovePlugin { .. }
                | Command::SaveText { .. }
                | Command::Components
                | Command::Pick { .. }
                | Command::BeginBatch
                | Command::EndBatch { .. }
                | Command::Undo
                | Command::Redo
                | Command::Render { .. }
                | Command::Save { .. }
                | Command::ExportObj { .. }
                | Command::ExportGlb { .. }
                | Command::ExportStl { .. }
                | Command::ExportDae { .. }
                | Command::ExportIfc { .. }
                | Command::Sheets { .. }
                | Command::Blueprint { .. }
                | Command::Levels
                | Command::Animate { .. }
                | Command::TimelineFrames { .. }
                | Command::VideoFrame { .. }
                | Command::VideoFinish { .. }
                | Command::ExportDwg { .. }
                | Command::ExportDxf { .. }
                | Command::ExportPdf { .. }
                | Command::Assets
        )
    }
}

#[derive(Default)]
pub struct Engine {
    pub model: Model,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Script names: `"as":"front"` on a command stores its result id,
    /// and any later `"@front"` value is replaced by it.
    pub names: HashMap<String, Id>,
    /// Component definition being edited (`edit`), if any.
    pub ctx: Option<Id>,
    /// bumps on every change to the model (progressive renders start over)
    revision: u64,
    /// a photoreal render in progress
    pt: Option<crate::pt::Progress>,
    /// memory undo history may use (estimated bytes); 0: the default
    pub undo_budget: usize,
    /// an open batch: the model before it began, the revision then, and when
    batch: Option<(Model, u64, std::time::Instant)>,
    /// counts batches, so an `end_batch` can say which one it means
    batch_seq: u64,
    /// the model was opened with more geometry than the limit allows: it can be looked at and
    /// cut down, but nothing that adds geometry runs and it can't be saved until it is under
    pub oversize: bool,
    /// the last Undo (true) or Redo (false) that was refused because of another program's change, and left
    /// its step in the list: asking again skips that step
    refused: Option<bool>,
    /// the command being run comes from an agent or a plugin (tighter limits); false: the person, whose importers
    /// and terrain may use the whole model budget
    untrusted: bool,
    /// set while `environment` runs its own fixed list of commands: they run as the person (the policy check passed for the
    /// whole), but keep the limits of whoever asked for it
    nested_untrusted: bool,
}

/// A batch nobody closed (the app went away mid-run) is closed after this (a plugin run is
/// limited to 60 s, a Ruby replay to the same).
pub const BATCH_MAX: std::time::Duration = std::time::Duration::from_secs(150);

/// The biggest model file `open` and Restore read (a model in memory is several times its file's size).
/// A model of the biggest size the program writes (`model::MAX_MODEL_BYTES`) is under this, with room: what the app
/// saved it can open again.
pub const MAX_OPEN_BYTES: u64 = 768 << 20;
/// What an agent's `open` reads: its own model cap (`model::MAX_MODEL_BYTES_UNTRUSTED`, 160 MB) plus the room a file has over
/// its estimate. A library part or a thumbnail is held to `library::MAX_LIBRARY_BYTES`.
pub const MAX_OPEN_BYTES_UNTRUSTED: u64 = 256 << 20;

const UNDO_LIMIT: usize = 200;
/// Undo history never holds more than about this much (per-step sizes are estimates).
const UNDO_BYTES: usize = 768 << 20;

fn v3(a: [f64; 3]) -> DVec3 {
    DVec3::from(a)
}

fn photo_px() -> u32 {
    1024
}

fn x_axis() -> [f64; 3] {
    [1.0, 0.0, 0.0]
}

fn z_axis() -> [f64; 3] {
    [0.0, 0.0, 1.0]
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run one JSON command line as the person using the app (no limits on
    /// which command, beyond the sanity checks) and return a JSON reply.
    pub fn exec_json(&mut self, line: &str) -> Value {
        self.exec_json_as(&Caller::Ui, line)
    }

    /// Run one JSON command line for `caller` (see `policy`). Never panics:
    /// bad input is an error reply, and a bug inside a command is an error
    /// reply with the model put back as it was.
    pub fn exec_json_as(&mut self, caller: &Caller, line: &str) -> Value {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return json!({"ok": false, "error": format!("bad JSON: {e}")}),
        };
        self.exec_value_as(caller, v)
    }

    pub fn exec_value_as(&mut self, caller: &Caller, v: Value) -> Value {
        // a panic anywhere (not only inside a command) is an error reply
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.exec_value_inner(caller, v))).unwrap_or_else(|_| json!({"ok": false, "error": "SketchCraft hit a bug running that."}))
    }

    fn exec_value_inner(&mut self, caller: &Caller, mut v: Value) -> Value {
        if self.batch.as_ref().is_some_and(|b| b.2.elapsed() > BATCH_MAX) {
            self.end_batch();
        }
        let name = v.as_object_mut().and_then(|o| o.remove("as")).and_then(|n| n.as_str().map(String::from));
        if let Err(e) = crate::policy::vet(caller, &mut v) {
            return json!({"ok": false, "error": e});
        }
        self.untrusted = !matches!(caller, Caller::Ui) || self.nested_untrusted;
        if let Err(e) = self.resolve(&mut v) {
            crate::policy::settle(caller, false);
            return json!({"ok": false, "error": e.to_string()});
        }
        let r = match serde_json::from_value::<Command>(v) {
            Ok(cmd) => self.exec(cmd),
            Err(e) => {
                crate::policy::settle(caller, false);
                return json!({"ok": false, "error": format!("bad command: {e}")});
            }
        };
        crate::policy::settle(caller, r["ok"] == true);
        if let (Some(n), true) = (name, r["ok"] == true) {
            if let Some(id) = ["face", "group", "instance", "guide", "dimension", "text"]
                .iter()
                .find_map(|k| r[*k].as_u64())
                .or_else(|| r["ids"][0].as_u64())
                .or_else(|| r["edges"][0].as_u64())
            {
                self.names.insert(n, id as Id);
            }
        }
        r
    }

    /// Is a batch (see `BeginBatch`) open?
    pub fn in_batch(&self) -> bool {
        self.batch.is_some()
    }

    #[doc(hidden)]
    pub fn expire_batch_for_test(&mut self) {
        if let Some(b) = self.batch.as_mut() {
            b.2 = std::time::Instant::now() - BATCH_MAX - std::time::Duration::from_secs(1);
        }
    }

    /// Close a batch nobody closed in time (the page went away mid-run). True if one was closed.
    pub fn close_expired_batch(&mut self) -> bool {
        if self.batch.as_ref().is_some_and(|b| b.2.elapsed() > BATCH_MAX) {
            self.end_batch();
            return true;
        }
        false
    }

    /// Close a batch: everything since it began is ONE undo step.
    fn end_batch(&mut self) {
        if let Some((before, rev, _)) = self.batch.take() {
            if self.revision != rev {
                self.undo.push(Step::Snap(Box::new(before)));
                self.redo.clear();
                self.trim_undo();
            }
        }
    }

    /// Close an open batch now (the person went on working): what it did becomes one undo step.
    pub fn close_batch(&mut self) {
        self.end_batch();
    }

    /// Take back, without a trace in the history, whatever steps were added after the history had `depth` of them
    /// (their change is being thrown away, for instance because another program saved first): the model goes back
    /// to what it was before them.
    pub fn drop_steps_after(&mut self, depth: usize) {
        let mut before = None;
        while self.undo.len() > depth {
            if let Some(Step::Snap(m)) = self.undo.pop() {
                before = Some(*m);
            }
        }
        if let Some(m) = before {
            let next = self.model.next_id;
            self.model = m;
            self.model.next_id = self.model.next_id.max(next);
        }
    }

    /// The undo and redo lists in a form that survives the reload of a model somebody else saved
    /// (see `history`: nothing is worked out yet, so this is instant). The lists are taken out of the
    /// engine: give the result to `install_history` once the reload is over, whether it worked or not.
    pub fn history_for_reload(&mut self) -> Option<crate::history::History> {
        if self.undo.is_empty() && self.redo.is_empty() {
            return None;
        }
        let (undo, redo) = (std::mem::take(&mut self.undo), std::mem::take(&mut self.redo));
        Some(crate::history::carry(undo, redo, &self.model))
    }

    /// Put a carried history back after a reload. Returns how many steps could not be kept.
    pub fn install_history(&mut self, h: crate::history::History) -> (usize, usize) {
        self.undo = h.undo;
        self.redo = h.redo;
        self.refused = None;
        self.trim_undo();
        (h.dropped, h.dropped_redo)
    }

    /// A step that is not a whole-model copy is about to move through the model: the copy next to it on the other
    /// list (the redo step beyond an Undo, the undo step beyond a Redo) is paired with the model as it is right now, so
    /// that it survives another program's save as a set of changes instead of being let go (the copy alone says
    /// nothing about what it leads from once a patch sits between).
    fn pair_neighbour(&mut self, back: bool) {
        let other = if back { &mut self.redo } else { &mut self.undo };
        if let Some(Step::Snap(_)) = other.last() {
            if let Some(Step::Snap(m)) = other.pop() {
                let now = std::sync::Arc::new(self.model.clone());
                // Undo: the redo copy is the model one step ahead of this one; Redo: the undo copy is one step behind
                other.push(if back { Step::Stale { before: now, after: std::sync::Arc::new(*m) } } else { Step::Stale { before: std::sync::Arc::new(*m), after: now } });
            }
        }
    }

    /// Replace the whole model with `model` as one undoable step (a restore from a backup).
    pub fn replace_model(&mut self, model: Model) -> Result<()> {
        if self.batch.is_some() {
            bail!("a plugin is still running; try again when it has finished");
        }
        // a file that names points, placements or groups that aren't there would draw nothing but trouble
        if let Some(what) = model.broken_refs(false).first() {
            bail!("that file isn't a consistent model ({what} refers to something that isn't there), so it was not restored");
        }
        let mut model = model;
        model.repair_ids();
        model.vet_pictures();
        // a model over the size limits is restored for viewing and cutting down, like Open: the limits never block getting work back
        model.check_sane_with(false)?;
        let over = model.over_limits();
        // ids are never handed out twice: not after a restore either (an agent or another program may still hold the ones in use now)
        model.next_id = model.next_id.max(self.model.next_id);
        let before = std::mem::replace(&mut self.model, model);
        self.undo.push(Step::Snap(Box::new(before)));
        self.redo.clear();
        self.ctx = None;
        self.names.clear();
        self.oversize = over;
        self.revision += 1;
        self.trim_undo();
        Ok(())
    }

    /// How many steps Undo can go back.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Bumps on every change to the model (and undo/redo); a client can tell
    /// whether a command changed anything.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Build (or replace, or remove) the environment around the model: one
    /// undo step, in a locked group tagged "Environment".
    pub fn environment(&mut self, kind: &str) -> Result<Value> {
        let (was, outer) = (self.nested_untrusted, self.untrusted);
        self.nested_untrusted = outer;
        let r = self.environment_inner(kind);
        self.nested_untrusted = was;
        // the nested commands set the flag as they ran: it goes back to what the caller's own command had
        self.untrusted = outer;
        r
    }

    fn environment_inner(&mut self, kind: &str) -> Result<Value> {
        if self.ctx.is_some() {
            bail!("close the component you're editing first");
        }
        let before = self.model.clone();
        let depth = self.undo.len();
        let fail = |e: &mut Engine, msg: String| -> Result<Value> {
            e.model = before.clone();
            e.undo.truncate(depth);
            bail!(msg)
        };
        // the old one goes
        let old: Vec<Id> = self.model.groups.iter().filter(|(_, g)| g.name.starts_with("Environment") && g.parent.is_none()).map(|(&k, _)| k).collect();
        for g in &old {
            self.model.groups.get_mut(g).unwrap().locked = false;
        }
        if !old.is_empty() {
            self.model.delete(&old)?;
        }
        self.model.environment = None;
        self.model.active_group = None;
        if kind == "none" {
            self.undo.push(Step::Snap(Box::new(before)));
            return Ok(json!({"environment": null}));
        }
        // around what's there (or a 10 m square at the origin)
        let (lo, hi) = self.model.world_bounds().unwrap_or((DVec3::new(-5.0, -5.0, 0.0), DVec3::new(5.0, 5.0, 0.0)));
        let cmds = crate::environment::build(kind, lo, hi)?;
        let title = crate::environment::KINDS.iter().find(|k| k.0 == kind).map(|k| k.1.split(':').next().unwrap_or(kind)).unwrap_or(kind);
        let outer = self.model.begin_group(format!("Environment: {title}"));
        let saved_names = std::mem::take(&mut self.names);
        for c in cmds {
            let r = self.exec_json(&c.to_string());
            if r["ok"] != true {
                self.names = saved_names;
                return fail(self, format!("building the {kind} environment: {}", r["error"]));
            }
            // pieces made by end_group step back out to the environment group
            if self.model.active_group.is_none() {
                self.model.active_group = Some(outer);
            }
        }
        self.names = saved_names;
        self.model.active_group = None;
        // placed assets sit inside it too
        let loose: Vec<Id> = self.model.instances.iter().filter(|(k, i)| i.parent.is_none() && !before.instances.contains_key(k)).map(|(&k, _)| k).collect();
        self.model.set_parent(&loose, Some(outer))?;
        self.model.set_tag(&[outer], Some("Environment".into()))?;
        self.model.tags.entry("Environment".into()).or_insert(true);
        self.model.groups.get_mut(&outer).unwrap().locked = true;
        self.model.environment = Some(kind.to_string());
        // all of it is one undo step
        self.undo.truncate(depth);
        self.undo.push(Step::Snap(Box::new(before)));
        self.redo.clear();
        Ok(json!({"environment": kind, "group": outer}))
    }

    fn resolve(&self, v: &mut Value) -> Result<()> {
        match v {
            Value::String(s) if s.starts_with('@') => {
                let Some(id) = self.names.get(&s[1..]) else { bail!("unknown name {s}") };
                *v = json!(id);
            }
            Value::Array(a) => a.iter_mut().try_for_each(|x| self.resolve(x))?,
            Value::Object(o) => o.values_mut().try_for_each(|x| self.resolve(x))?,
            _ => {}
        }
        Ok(())
    }

    /// Run a command. A command that fails, panics, or leaves a non-finite
    /// or absurd number in the model changes nothing (and costs no undo step).
    pub fn exec(&mut self, cmd: Command) -> Value {
        let is_env = matches!(cmd, Command::Environment { .. });
        // opening a file replaces the model and the history with it: Undo must never
        // step back to a model that isn't the one on disk
        let is_open = matches!(cmd, Command::Open { .. });
        let mutates = cmd.mutates() || is_env;
        if mutates {
            self.refused = None;
        }
        if mutates || matches!(cmd, Command::Undo | Command::Redo) {
            self.revision += 1;
        }
        // everything needed to put things back as they were
        let saved = mutates.then(|| (self.model.clone(), self.ctx, self.names.clone(), self.undo.len()));
        let count_before = self.model.entity_count();
        let bytes_before = if mutates { self.model.size_estimate() } else { 0 };
        let text_before = if mutates { self.model.text_bytes() } else { 0 };
        let oversize_before = self.oversize;
        // how much one command may add: the person's importers and terrain may use up to the model's total; everything
        // else (repeat commands such as `array`, and anything an agent or a plugin runs) is held to MAX_GROWTH
        let grow_cap = if self.untrusted {
            crate::model::MAX_GROWTH
        } else if matches!(cmd, Command::ImportObj { .. } | Command::ImportSkp { .. } | Command::ImportDxf { .. } | Command::ImportDwg { .. } | Command::ImportFbx { .. } | Command::ImportIfc { .. } | Command::ImportCloud { .. } | Command::TerrainGrid { .. } | Command::TerrainFromContours { .. } | Command::Open { .. }) {
            crate::model::MAX_ENTITIES
        } else {
            crate::model::MAX_GROWTH
        };
        crate::model::set_growth_limit(grow_cap);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Command::Environment { kind } = &cmd {
                // pushes its own (single) undo step
                return self.environment(kind).map(|mut v| {
                    v["ok"] = json!(true);
                    (v, false)
                });
            }
            crate::safety::take_skipped();
            self.run(cmd).map(|mut v| {
                v["ok"] = json!(true);
                // pictures left out of a render, an import or a projection are said, never dropped silently
                let skipped = crate::safety::take_skipped();
                if !skipped.is_empty() && v.is_object() {
                    v["skipped_pictures"] = json!(skipped);
                }
                (v, mutates)
            })
        }));
        let restore = |e: &mut Engine, saved: Option<(Model, Option<Id>, HashMap<String, Id>, usize)>| {
            if let Some((model, ctx, names, depth)) = saved {
                e.model = model;
                e.ctx = ctx;
                e.names = names;
                e.undo.truncate(depth);
            }
        };
        match outcome {
            Ok(Ok((v, push_undo))) => {
                if mutates {
                    // a model over the geometry limit (opened that way) may only get smaller; one command may not add more than MAX_GROWTH
                    let count = self.model.entity_count();
                    let bytes = self.model.size_estimate();
                    let text = self.model.text_bytes();
                    let within = count <= crate::model::MAX_ENTITIES && bytes <= crate::model::MAX_MODEL_BYTES && text <= crate::model::MAX_TEXT_BYTES && !self.model.attrs_over();
                    let checked = if is_open && self.oversize {
                        self.model.check_sane_with(false)
                    } else if self.oversize && within {
                        self.oversize = false;
                        self.model.check_sane_est(bytes)
                    } else if self.oversize {
                        self.model.check_sane_with(false).and_then(|_| if count > count_before || bytes > bytes_before || text > text_before { Err(anyhow::anyhow!("this model is over the size limits ({} points, edges and faces; about {} MB), so it is open for viewing and for deleting parts only; that would add more", count_before, bytes_before >> 20)) } else { Ok(()) })
                    } else if !is_open && count > count_before.saturating_add(grow_cap) {
                        Err(anyhow::anyhow!("that would add {} points, edges and faces at once (the limit is {}), so it was not done", count - count_before, grow_cap))
                    } else if !is_open && self.untrusted && bytes > bytes_before.saturating_add(crate::model::MAX_GROWTH_BYTES_UNTRUSTED) {
                        Err(anyhow::anyhow!("that would add {} MB of text, images, attributes or scan points at once (the limit for an agent or a plugin is {} MB), so it was not done", (bytes - bytes_before) >> 20, crate::model::MAX_GROWTH_BYTES_UNTRUSTED >> 20))
                    } else if !is_open && self.untrusted && text > crate::model::MAX_TEXT_BYTES_UNTRUSTED && text > text_before {
                        Err(anyhow::anyhow!("the model's names, notes and attributes are about {} MB; an agent or a plugin can't add more past {} MB (the person's own naming has the rest of the room), so it was not done ({})", text >> 20, crate::model::MAX_TEXT_BYTES_UNTRUSTED >> 20, self.model.text_use()))
                    } else if !is_open && self.untrusted && bytes > crate::model::MAX_MODEL_BYTES_UNTRUSTED && bytes > bytes_before {
                        Err(anyhow::anyhow!("the model is about {} MB; an agent or a plugin can't make it bigger past {} MB (the person can), so it was not done", bytes >> 20, crate::model::MAX_MODEL_BYTES_UNTRUSTED >> 20))
                    } else {
                        self.model.check_sane_est(bytes)
                    };
                    if let Err(e) = checked {
                        restore(self, saved);
                        self.oversize = oversize_before;
                        return json!({"ok": false, "error": e.to_string()});
                    }
                    if is_open {
                        self.undo.clear();
                    } else if push_undo && self.batch.is_none() {
                        if let Some((before, ..)) = saved {
                            self.undo.push(Step::Snap(Box::new(before)));
                        }
                    }
                    self.redo.clear();
                    self.trim_undo();
                }
                v
            }
            Ok(Err(e)) => {
                restore(self, saved);
                json!({"ok": false, "error": e.to_string()})
            }
            Err(panic) => {
                let msg = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown".into());
                eprintln!("sketchcraft: a command panicked: {msg}");
                restore(self, saved);
                if msg.starts_with(crate::model::IDS_OUT) {
                    return json!({"ok": false, "error": "this model has used up its ids (the counter is at the top of its range), so nothing more can be added; export it (OBJ or SketchUp) and import it into a new model. Nothing was changed."});
                }
                json!({"ok": false, "error": "SketchCraft hit a bug running that. Nothing was changed."})
            }
        }
    }

    /// Keep undo history within `UNDO_LIMIT` steps and `UNDO_BYTES` of memory
    /// (the newest step is always kept).
    fn trim_undo(&mut self) {
        let mut total: usize = self.undo.iter().map(Step::approx_bytes).sum();
        while self.undo.len() > UNDO_LIMIT || (self.undo.len() > 1 && total > if self.undo_budget == 0 { UNDO_BYTES } else { self.undo_budget }) {
            total = total.saturating_sub(self.undo[0].approx_bytes());
            self.undo.remove(0);
        }
        // what Redo holds counts against the same memory (patches are small; copies are not)
        let limit = if self.undo_budget == 0 { UNDO_BYTES } else { self.undo_budget };
        let mut redo_total: usize = self.redo.iter().map(Step::approx_bytes).sum();
        while self.redo.len() > 1 && redo_total > limit {
            redo_total = redo_total.saturating_sub(self.redo[0].approx_bytes());
            self.redo.remove(0);
        }
    }

    /// Undo (`back`) or Redo one step. A step taken since the last reload is a copy of the model; one taken before
    /// another program saved is worked out now (see `history`) and applied on top of the model as it is, touching only
    /// what that step changed. If another program's change stands in the way the step is refused with a reason and
    /// stays in the list; asking again skips it and goes one step further.
    fn step_history(&mut self, back: bool) -> Result<Value> {
        let verb = if back { "Undo" } else { "Redo" };
        let mut skipped = 0;
        loop {
            let step = if back { self.undo.pop() } else { self.redo.pop() };
            let Some(step) = step else {
                if skipped > 0 {
                    self.refused = None;
                    bail!("{verb}: {skipped} step{} couldn't be {} because another program changed what {} touched, and there is nothing further {}", if skipped == 1 { "" } else { "s" }, if back { "taken back" } else { "done again" }, if skipped == 1 { "it" } else { "they" }, if back { "back" } else { "ahead" });
                }
                bail!("nothing to {}", verb.to_lowercase())
            };
            let step = match step {
                Step::Snap(other) => {
                    // a step taken since the last reload: the model is exactly as that step left it
                    let next = self.model.next_id;
                    let was = std::mem::replace(&mut self.model, *other);
                    // ids are never handed out twice: not after Undo either (another program may have used the ones this step freed)
                    self.model.next_id = self.model.next_id.max(next);
                    let was = Step::Snap(Box::new(was));
                    if back { self.redo.push(was) } else { self.undo.push(was) }
                    self.refused = None;
                    return Ok(json!({}));
                }
                Step::Stale { before, after } => {
                    self.pair_neighbour(back);
                    let p = crate::history::patch_between(&before, &after);
                    if p.is_empty() {
                        // it changed nothing in the model's content (a view, a selection): not a step
                        continue;
                    }
                    p
                }
                Step::Patch(p) => {
                    self.pair_neighbour(back);
                    p
                }
            };
            match step.apply(&self.model, !back) {
                Ok(m) => {
                    let next = self.model.next_id;
                    self.model = m;
                    self.model.next_id = self.model.next_id.max(next);
                    if back { self.redo.push(Step::Patch(step)) } else { self.undo.push(Step::Patch(step)) }
                    self.refused = None;
                    return Ok(if skipped > 0 { json!({"skipped": skipped}) } else { json!({}) });
                }
                Err(why) => {
                    if self.refused == Some(back) {
                        // asked again after a refusal: this step is let go and the next one is tried
                        self.refused = None;
                        skipped += 1;
                        continue;
                    }
                    // kept: the other program may take its change back, and then this works
                    if back { self.undo.push(Step::Patch(step)) } else { self.redo.push(Step::Patch(step)) }
                    self.refused = Some(back);
                    bail!("{verb} stopped: this step can't be {} right now because {why} (another program changed it). The step is kept: try again after that program's change is taken back, or press {verb} again to skip this step and go one {}.", if back { "taken back" } else { "done again" }, if back { "further back" } else { "further ahead" });
                }
            }
        }
    }

    fn run(&mut self, cmd: Command) -> Result<Value> {
        // inside `edit`, geometry commands work on the component's definition
        if let Some(d) = self.ctx {
            if !self.model.definitions.contains_key(&d) {
                self.ctx = None;
            }
        }
        // each area's arms live in commands/<area>.rs
        match cmd {
            c @ (
                Command::New { .. } | Command::Rect { .. } | Command::Polygon { .. } | Command::Line { .. } |
                Command::Circle { .. } | Command::Arc { .. } | Command::Offset { .. } | Command::FollowMe { .. } |
                Command::Pushpull { .. } | Command::Move { .. } | Command::Rotate { .. } | Command::Scale { .. } |
                Command::Duplicate { .. } | Command::Array { .. } | Command::Delete { .. } |
                Command::Reverse { .. } | Command::RaiseWalls { .. } | Command::Weld { .. } |
                Command::ExplodeCurve { .. } | Command::OrientFaces { .. } | Command::Text3d { .. } |
                Command::Subd { .. } | Command::CageEdit { .. } | Command::Subdivide { .. } |
                Command::DoorSwing { .. } | Command::TerrainFromContours { .. } | Command::TerrainGrid { .. } |
                Command::Smoove { .. } | Command::Stamp { .. } | Command::Drape { .. } | Command::FlipEdge { .. } |
                Command::Bevel { .. } | Command::Divide { .. } | Command::Slice { .. } | Command::Intersect { .. } |
                Command::Flip { .. } | Command::Solid { .. } | Command::Volume { .. } | Command::Wall { .. } |
                Command::Stairs { .. } | Command::Roof { .. } | Command::Column { .. } | Command::Insert { .. } |
                Command::Assets { .. }
            ) => self.run_draw(c),
            c @ (
                Command::BeginGroup { .. } | Command::EndGroup { .. } | Command::Group { .. } |
                Command::Faces { .. } | Command::Pick { .. } | Command::Copy { .. } | Command::MakeComponent { .. } |
                Command::Place { .. } | Command::ComponentFaceMe { .. } | Command::ComponentCuts { .. } |
                Command::Edit { .. } | Command::Close { .. } | Command::Explode { .. } | Command::Components { .. } |
                Command::Hide { .. } | Command::Unhide { .. } | Command::Tag { .. } | Command::TagVisible { .. } |
                Command::TagColor { .. } | Command::RenameTag { .. } | Command::SetParent { .. } |
                Command::Ungroup { .. } | Command::Lock { .. } | Command::Soften { .. } | Command::Rename { .. } |
                Command::MakeUnique { .. } | Command::Opacity { .. } | Command::Connected { .. } |
                Command::SameAs { .. } | Command::Resize { .. } | Command::SetAxes { .. } |
                Command::SetAttrs { .. } | Command::Classify { .. } | Command::Reveal { .. }
            ) => self.run_structure(c),
            c @ (
                Command::Paint { .. } | Command::Texture { .. } | Command::Photoreal { .. } |
                Command::Render { .. } | Command::Environment { .. } | Command::Sun { .. } |
                Command::Section { .. } | Command::SectionPlane { .. } | Command::SectionEdit { .. } |
                Command::SectionOff { .. } | Command::AddLocation { .. } | Command::Thumbnail { .. } |
                Command::Purge { .. } | Command::DeleteMaterial { .. } | Command::PurgeMaterials { .. } |
                Command::Skins { .. } | Command::SaveSkin { .. } | Command::Material { .. } |
                Command::SaveImage { .. }
            ) => self.run_look(c),
            c @ (
                Command::Guide { .. } | Command::Dimension { .. } | Command::Text { .. } | Command::Units { .. } |
                Command::ClearGuides { .. } | Command::ShowView { .. } | Command::SaveView { .. } |
                Command::MatchPhoto { .. } | Command::DeleteView { .. } | Command::Parts { .. } |
                Command::Info { .. } | Command::ProjectPhoto { .. } | Command::PhotoSolve { .. } |
                Command::Finish { .. } | Command::Options { .. } | Command::Report { .. }
            ) => self.run_annotate(c),
            c @ (
                Command::Plugins { .. } | Command::PluginEnable { .. } | Command::InstallPlugin { .. } |
                Command::RemovePlugin { .. } | Command::SaveText { .. } | Command::Save { .. } |
                Command::Open { .. } | Command::ExportGlb { .. } | Command::ExportStl { .. } |
                Command::Animate { .. } | Command::Blueprint { .. } | Command::Levels { .. } |
                Command::Timeline { .. } | Command::TimelineFrames { .. } | Command::VideoFrame { .. } |
                Command::VideoFinish { .. } | Command::ImportDxf { .. } | Command::ImportDwg { .. } |
                Command::ExportDwg { .. } | Command::Sheets { .. } | Command::ExportIfc { .. } |
                Command::ExportDae { .. } | Command::ExportDxf { .. } | Command::ExportPdf { .. } |
                Command::ImportObj { .. } | Command::ImportSkp { .. } | Command::ImportCloud { .. } |
                Command::CloudData { .. } | Command::Cloud { .. } | Command::ImportIfc { .. } |
                Command::Library { .. } | Command::InsertFile { .. } | Command::SaveToLibrary { .. } |
                Command::LibraryAdd { .. } | Command::ImportFbx { .. } | Command::ExportObj { .. }
            ) => self.run_files(c),
            c @ (
                Command::BeginBatch { .. } | Command::EndBatch { .. } | Command::Undo { .. } | Command::Redo { .. }
            ) => self.run_session(c),
        }
    }
}

/// After placing: a component that cuts openings, put on a face, cuts one.
/// What happened, for the reply (null when nothing was asked).
fn place_cut(m: &mut Model, instance: Id, host: Option<Id>) -> Value {
    let Some(host) = host else { return Value::Null };
    let Some(i) = m.instances.get(&instance) else { return Value::Null };
    let (def_id, t) = (i.def, i.transform);
    let Some(d) = m.definitions.get(&def_id).filter(|d| d.cuts) else { return Value::Null };
    let def = d.model.clone();
    match crate::openings::cut(m, &def, t, host) {
        Ok(n) => json!({"faces": n}),
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// A readable name from a download's file name: "uploads_files_2787791_Mercedes+Benz+GLS+580"
/// -> "Mercedes Benz GLS 580".
fn tidy_name(stem: &str) -> String {
    let s = stem.replace(['+', '_'], " ");
    let words: Vec<&str> = s.split_whitespace().collect();
    // drop store prefixes like "uploads files 2787791"
    let start = words.iter().position(|w| !matches!(w.to_ascii_lowercase().as_str(), "uploads" | "files" | "upload" | "file") && !w.chars().all(|c| c.is_ascii_digit())).unwrap_or(0);
    let name = words[start..].join(" ");
    if name.is_empty() { stem.to_string() } else { name }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CageOp {
    InsertLoop { edge: [usize; 2], #[serde(default = "half")] t: f64 },
    Extrude { faces: Vec<usize>, distance: f64 },
    DeleteFaces { faces: Vec<usize> },
    Merge { points: Vec<usize> },
}

fn half() -> f64 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
pub struct KnownLength {
    pub a: usize,
    pub b: usize,
    pub length: f64,
}

#[derive(serde::Serialize)]
struct SunAt {
    azimuth: f64,
    altitude: f64,
    lat: f64,
    lon: f64,
    /// false: no location set, so a default (40 N, 98 W) was used
    located: bool,
    utc_offset: f64,
}

/// The real sun for the model's location at a local date ("2026-06-21") and time ("15:30").
fn sun_at(model: &Model, date: Option<&str>, time: Option<&str>, utc_offset: Option<f64>) -> Result<SunAt> {
    let located = model.location.is_some();
    let (lat, lon) = model.location.as_ref().map(|l| (l.lat, l.lon)).unwrap_or((40.0, -98.0));
    let parts: Vec<&str> = date.unwrap_or("2026-06-21").split('-').collect();
    let num = |i: usize| parts.get(i).and_then(|s| s.trim().parse::<i64>().ok());
    let (Some(y), Some(mo), Some(da)) = (num(0), num(1), num(2)) else { bail!("date is year-month-day, like 2026-06-21") };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
        bail!("date is year-month-day, like 2026-06-21");
    }
    let hm: Vec<&str> = time.unwrap_or("15:00").split(':').collect();
    let Some(h) = hm.first().and_then(|s| s.trim().parse::<f64>().ok()) else { bail!("time is hours:minutes, like 15:30") };
    let m = hm.get(1).and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0);
    let utc_offset = utc_offset.unwrap_or((lon / 15.0).round());
    let (azimuth, altitude) = crate::pt::solar_position(lat, lon, y as i32, mo as u32, da as u32, h + m / 60.0, utc_offset);
    Ok(SunAt { azimuth, altitude, lat, lon, located, utc_offset })
}

/// The sun for a sun-study frame: [azimuth, altitude] from the model's location.
fn timeline_sun(model: &Model, date: &str, time: &str) -> Result<[f64; 2]> {
    let s = sun_at(model, Some(date), Some(time), None)?;
    Ok([s.azimuth, s.altitude])
}

/// Where a video's frames wait to be encoded (a session name is letters and digits only).
fn video_dir(session: &str) -> Result<std::path::PathBuf> {
    if session.is_empty() || session.len() > 40 || !session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("bad video session name");
    }
    Ok(crate::safety::cache_dir("video").join(session))
}

/// Where app skins live: ~/Documents/SketchCraft/Skins.
fn skins_dir() -> std::path::PathBuf {
    crate::library::default_dir().parent().map(|p| p.join("Skins")).unwrap_or_else(|| std::path::PathBuf::from("Skins"))
}
