# Opal (vendored)

SketchCraft runs SketchUp Ruby scripts by compiling them to JavaScript with
[Opal](https://opalrb.com) (MIT), inside the plugin sandbox.

| File | From | Version |
| --- | --- | --- |
| `opal.js` | npm `opal-runtime` (`src/opal.js`) | 3.0.0 (Opal 1.7.3) |
| `opal-compiler.js` | npm `opal-compiler` (`src/opal-builder.js`) | 3.0.0 (Opal 1.7.3) |

Licenses: `LICENSE-opal-runtime`, `LICENSE-opal-compiler`. Only the compiler
(`opal/compiler`) and the core runtime are used; nothing is loaded at run time
from the network.
