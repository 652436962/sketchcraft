# Writing a SketchCraft plugin

A plugin adds commands to SketchCraft. It's a folder with two files:

```
~/Documents/SketchCraft/Plugins/
  my-plugin/
    plugin.json
    main.js
```

**File › Plugins ▸ Open plugins folder…** opens that folder. New plugins show up
the next time you open the Plugins menu, and in Shift+S.

## plugin.json

```json
{
  "name": "Board feet",
  "version": "1.0.0",
  "author": "You",
  "description": "Totals the board feet of the selected parts.",
  "commands": [
    { "id": "board-feet", "label": "Board feet…", "hint": "Board feet of the selection" }
  ]
}
```

### A tool bar button

Give a command an `"icon"` and it gets a button on the tool bar (in its own
group at the end, kept visible when the bar is short of room):

```json
{ "id": "board-feet", "label": "Board feet…", "icon": "board-feet.svg" }
```

The icon is an SVG or PNG file beside `main.js` (up to 256 KB), or SVG written
inline (`"icon": "<svg xmlns=…>…</svg>"`). Draw it on a 24 × 24 grid; a
mid-grey stroke reads on light and dark themes.

## main.js

```js
sketchcraft.command("board-feet", async (ctx) => {
  const ids = [...ctx.selection.groups, ...ctx.selection.instances];
  const { parts } = await ctx.run({ cmd: "parts", ids });
  const inches = (m) => m / 0.0254;
  const bf = parts.reduce((t, p) => t + (inches(p.size[0]) * inches(p.size[1]) * inches(p.size[2])) / 144, 0);
  ctx.show({
    title: "Board feet",
    text: `${bf.toFixed(1)} board feet in ${parts.length} parts.`,
    table: { columns: ["Part", "Board feet"], rows: parts.map((p) => [p.name, ((inches(p.size[0]) * inches(p.size[1]) * inches(p.size[2])) / 144).toFixed(2)]) },
  });
});
```

The built-in **Cut list** plugin is a longer example:
[core/plugins/cutlist/main.js](../core/plugins/cutlist/main.js).

## The API

`sketchcraft.command(id, fn)` adds a command. `fn(ctx)` runs when it's picked,
and can be `async`. `ctx` has:

| | |
| --- | --- |
| `ctx.selection` | `{ faces, edges, groups, instances }`: ids of what's selected |
| `ctx.units` | the model's units: `"m"`, `"cm"`, `"mm"`, `"ft"` or `"in"` (sizes are always meters) |
| `ctx.run(cmd)` | run an engine command, returns its reply (`{ ok, ... }` or `{ ok: false, error }`) |
| `ctx.show({ title, text, table })` | show a result; `table` is `{ columns: [...], rows: [[...], ...] }`, which people can copy or save as CSV |
| `ctx.toast(text)` | a short message at the top of the model |

`ctx.run` takes the same commands as the CLI and the MCP server: draw, push/pull,
move, paint, group, place components and more (see the README's command
reference). Changes a plugin makes can be undone like any other. Two read-only
commands are handy for plugins:

- `{"cmd": "parts", "ids": [...]}`: each group's or component's name, material
  and `size` as `[length, width, thickness]` in meters, measured along its own
  faces (so a turned board reads right). No ids: every group with nothing
  grouped inside it, and every component.
- `{"cmd": "info"}`: counts, groups and materials.

## What plugins can't do

Each run gets a fresh, sandboxed worker:

- no access to the page, the network (`fetch` and friends are gone) or your files;
- commands that read or write files (`save`, `open`, `import_*`, `export_*`,
  anything with a `path` or `out`, and the like) are refused;
- a run that takes over a minute is stopped.

So a plugin can change your model (and you can undo that), but it can't send
anything anywhere or touch other files.
