# AI agents over MCP

`sketchcraft mcp` is a [Model Context Protocol](https://modelcontextprotocol.io)
server with three tools:

| Tool | Does |
| --- | --- |
| `sketchcraft_run` | runs commands (one, or a list) and returns the replies |
| `sketchcraft_render` | renders the model and returns the image, so the agent can look at what it built |
| `sketchcraft_info` | what's in the model |

## Working alongside the app

Give `--doc` the desktop app's live file and you and the agent edit the same
model: what it builds appears in the app within a second, and what you do is
there for it.

```sh
sketchcraft mcp --doc ~/Documents/SketchCraft/live.sketchcraft
```

The live file is `~/Documents/SketchCraft/live.sketchcraft`, or the path in the
`SKETCHCRAFT_DOC` environment variable if you set one. The app and the agent
each pick up the other's latest save before they change anything, and save the
whole model after a change, so edits don't overwrite each other (if you both
change the same thing at the same moment, the later one wins). Backups of the
file are kept in `Backups/` beside it.

## What an agent may do

An agent isn't you, and a model can carry words written by someone else (names
in an imported file can read like instructions). So an agent gets less than the
person at the keyboard:

- modeling, inspection and rendering, and file import/export, but only **inside
  a workspace folder**: the model's folder by default, `--workspace DIR` to
  choose another, `--any-path` to lift the limit. Relative paths start there;
  hidden files, `Backups/` and `Plugins/` are off limits;
- never: installing, enabling or removing plugins, `save_text` (writing any text
  to any file), `reveal`, `skins`, `video_frame`/`video_finish`;
- numbers outside +/-1e9, over-long strings and URL or pipe paths are refused
  for everyone;
- names and text read from the model are marked as data in the tool output.

## Claude Code

This repository's `.mcp.json` points Claude Code at a local build under
`${HOME}/Documents/Companies/sketchcraft/target/release/` (it runs that binary
when you trust the project, so only trust a clone whose binary you built). In another
project, add:

```json
{
  "mcpServers": {
    "sketchcraft": {
      "command": "/path/to/sketchcraft",
      "args": ["mcp", "--doc", "/Users/you/Documents/SketchCraft/live.sketchcraft"]
    }
  }
}
```

The commands an agent can send are the same as for scripts: see
[Scripting](scripting.md).
