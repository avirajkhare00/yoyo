# ONNX Runtime Assets

This directory is the source of truth for reusable ONNX Runtime build assets that yoyo release CI consumes.

Workflow:

1. Update `assets.json` when the ONNX Runtime version, target, or asset naming changes.
2. Run the `Build ONNX Runtime Asset` workflow to publish the named asset once.
3. Cut yoyo releases normally. `release.yml` and `homebrew-install.yml` will download the stored asset instead of rebuilding ONNX Runtime on every Intel macOS job.

These assets live as GitHub release assets, not workflow artifacts, so they remain durable across runs.
