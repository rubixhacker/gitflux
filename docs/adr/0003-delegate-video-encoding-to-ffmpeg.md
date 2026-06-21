# Delegate video encoding to FFmpeg

Gitflux will own the video export workflow, including frame timing, render scheduling, progress, presets, and deterministic frame generation, while delegating media encoding to an installed FFmpeg binary. This keeps the product focused on fast Git history visualization and avoids making codec support, muxing, hardware acceleration, and FFmpeg library packaging part of the core architecture.
