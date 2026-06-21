# Use direct wgpu rendering

Gitflux will build its renderer directly on `wgpu` rather than adopting a game engine or 2D-only renderer as the core rendering architecture. The project needs deterministic offscreen frame production for CLI-driven video export, explicit GPU control, and portability across modern native graphics backends without inheriting an engine's desktop game-loop assumptions.
