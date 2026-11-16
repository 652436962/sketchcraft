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
each save the whole model after a change, and the last save wins, so take turns
rather than editing at the same moment.

## Claude Code

This repository's `.mcp.json` points Claude Code at a local build. In another
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
