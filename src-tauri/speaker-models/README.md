# Speaker models (bundled)

The two speaker models that speaker colors use are shipped inside the installers, in this folder (Windows: next to
`overlaysubs.exe` in `speaker-models\`; macOS: `Contents/Resources/speaker-models/`). Their pinned URLs, sizes and
SHA-256 values are the `speaker` and `segmentation` entries of `models.json`; the `.onnx` files are not committed.

Fetch them before building (release CI does the same, and fails on a checksum mismatch):

    python3 scripts/stage-speaker-models.py

Licenses: `LICENSE-3dspeaker-campplus.txt` (Apache-2.0) and `LICENSE-pyannote-segmentation.txt` (MIT), installed
next to the models.
