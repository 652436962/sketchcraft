# 10. Troubleshooting and questions

**The Mac says SketchCraft can't be opened, or Apple can't check it.**
Open **System Settings › Privacy & Security**, scroll down, and click **Open
Anyway**. You only do this once.

**Windows says "Windows protected your PC".** Click **More info › Run anyway**.
SketchCraft isn't signed by Microsoft yet.

**Is it free?** Yes, and open source (MIT license).

**Does it open my SketchUp files?** Yes: SketchUp 6 to 8 and 2013 onward. It
skips what it can't read (dimensions and text inside the file) rather than
guess.

**DWG files won't open.** Install a DWG helper (LibreDWG or the ODA File
Converter); see [Files](05-files.md). DXF works without one.

**My lines don't make a face.** Faces need a closed loop of edges that all lie
flat in one plane. Zoom in on the corners: a tiny gap or a point off the plane
stops it. Drawing with snapping (green endpoint dot) closes loops exactly.

**Push/Pull carved into the wrong thing.** Something else was stuck to that
face. Make each part a group so parts don't stick together.

**The model is slow.** Turn off Shadows and ambient occlusion while you work,
hide what you don't need (tags help), and use components for repeated things.

**Where are my files?** Wherever you saved them. SketchCraft's own folder is
`Documents/SketchCraft`: the live model, themes and plugins.

**How do I update?** It updates itself from 0.3.3; **File › Check for updates…**
checks now. The portable Windows ZIP points you to the new ZIP instead.

**Something's wrong.** Please [open an issue](https://github.com/jasonsutter87/sketchcraft/issues)
with what you did and what happened; a screenshot or the model helps a lot.
