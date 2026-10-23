# HomeBots – MODLOG

Journal for the HomeBots rewrite (Homefront 2011, UE3 → Rust/Bevy, reads the user's own install).
Route: **reimplement** (IW4L/hl2-rs style) – own engine code, game data read at runtime from the
user's install, nothing from the game is committed. Decompiles/captures stay outside the repo.

## Install / versions
- Steam install: `C:\Program Files (x86)\Steam\steamapps\common\HOMEFRONT` (Binaries\HOMEFRONT.exe, 32-bit)
- Packages: UE3 version 576, licensee 100, LZO chunked. Game settings:
  `Documents\My Games\HOMEFRONT\GCGame\Config\GCEngine.ini` (AllowD3D10=True → game renders with D3D11/SM4).
- Bevy 0.16.1, Rust stable.

## Package format (verified)
- Export entry 17 dwords (no component map; net-object list; GUID + package flags).
- Tagged props: ByteProperty size 8 = FName, FColor stored BGRA, bool = u32 in the tag.
- Texture2D bulk flags: 0x1 separate file, 0x10 LZO, 0x20 unused; separate-file mips read from the
  outermost source package at absolute offset.
- StaticMesh: map copies keep only the lowest LOD data; LOD header vertex counts locate the streams.
- Terrain: grid (SizeX·Tess+1)×(SizeY·Tess+1) u16; DrawScale3D default (64,64,256) (Engine.u
  Default__Terrain); Z = loc.z + (h−32768)·256/128; weightmap channels A,R,G,B for AlphaMapIndex 0..3.
- Struct defaults (e.g. PostProcessSettings) are a tagged-property block inside the ScriptStruct export
  (example `structdef`).

## Lighting units (Bevy source)
- exposure = 2^-ev100 / 1.2; ev100 = 0; directional illuminance = UE Brightness·π·1.2 → Bevy output equals
  UE3 albedo·brightness·NdotL. SkyLight = hemisphere lerp(lower, upper, N.y·0.5+0.5).

## Post-process (measured, apitrace 2026-10-15)
- Capture: apitrace 14.0 x86 d3d9 wrapper; game forced to D3D9 with AllowD3D10=False (restored after);
  tutorial SP-Devil, 1366×768, 5634 frames, 3.7 GB. Analysis: apitrace built from source on Linux,
  `apitrace dump --blobs`, shaders disassembled with own SM3 disassembler (scratchpad/sm3dis.py).
- Mapping confirmed against SP-Devil PostProcessVolume_2 (and _3):
  HDRLumParameters = (LowClamp, HighClamp, Center, Factor); LumAdaptTime = HDRLumTime;
  BloomScale = Bloom_Scale; SceneShadowsAndDesaturation.w = 1−Desat; SceneScaledLuminanceWeights =
  Desat·(0.3,0.59,0.11); GammaColorScaleAndInverse = (1,1,1,1/2.2); GammaOverlayColor = 0;
  VignetteScaleBias = (GradientScale, GradientOffset).
- Pass chain: lum 16×16 ← scene, 4×4, 1×1 (R32F) → adapt (ping-pong) → DOF (not done) → bloom
  downsample into (W/4+2)×(H/4+2) A16B16G16R16 → blur H/V → uber → back buffer.
- Game bug kept: luminance SampleOffsets uploaded packed (float2) but read one per register → half the
  taps + garbage tail (incl. NaN, 4.4e18). NaN tap taken as u=0 (only unmeasurable bit).
- Blur kernel: K=16 → taps −5..6, σ=2.0645825 tap units; K=20 → taps −6..7, σ=2.3082733; offsets ×
  BlurStepSize. Fits capture < 1e-6 (unit test in hfpost.rs).
- SM4 (D3D11) uber shader math identical to SM3 (checked via vkd3d-compiler → spirv-cross).

## Terrain (verified, 2026-10-15)
- Shader cache record layout: TMap<FStaticParameterSet, FMaterialShaderMap>; per entry: key | SkipOffset |
  shaders | mesh maps (count + (FName, GUID, FName) entries + VF FName) | MaterialId | FriendlyName | ...
  The friendly name FOLLOWS the shader lists it belongs to (earlier parser attributed lists to the next record).
- Devil capture: terrain = lighting pass (MatType:1, FTerrainLightingPassVertexFactory base pass, writes
  light rgb + spec a) then colour pass (MatType:0, FTerrainColorPixelShader) with DESTALPHA/SRCCOLOR blend
  → final = colour*(light+spec). MatType:0 colour PS contains the overlay fade ((w-3000)*0.001).
- VS constants from HOMEFRONT.exe FUN_00e185b0 (SetMesh) + capture: InvMaxTesselationLevel_ZScale =
  (32/MaxTessellationLevel, 1/128), InvTerrainSize_SectionBase = (1/NumVerticesX/Y, SectionBase),
  WeightMapDistort = (MinX, MinY, 1/(MaxX-MinX), 1/(MaxY-MinY)), LocalToWorld translation includes
  SectionBase*scale, TerrainHeight = 0.
- Terrain native: u32,u32, TArray<7-byte section entry> (byte 6 = palette, x fastest; checked against
  road/sign positions), TArray<weightmap>, FTerrainMaterialResources (uniform expressions).
- Quad diagonal: game uses dynamic index buffers; shorter height diagonal matches 93 % (component_94).
  Exact rule not found (open).

## Level objects (2026-10-15)
- Sublevels import meshes exported by FL-Overpass.map (e.g. STA_AngelIsland.rocks.Boulder8, 186x):
  resolve imports by object path across all loaded maps.
- Archetypes: placed actors/components serialise only deltas → merge props over the archetype chain.
- SkeletalMesh (v576 Homefront): sections 10 bytes (u16 mat, u16 chunk, u32 base, u16 tris), index
  buffer (elemsize, count), GPU skin buffer (stride, count, stride, count): pos, TangentX, TangentZ, 4 bytes,
  bones, weights, half2 UVs. Ref pose; LocalToWorld = RT(RotOrigin, Origin) * actor (vans upright).
- RoadSplineMeshComponent: FSplineMeshVertexFactory VS transcribed (Hermite with StartPos 0, frame from
  D x Z, roll/scale lerp). Segment end = next RoadSpline Location (checked). SplineInfo = (MinY,
  1/LengthY) and SplineEndPos.w = MaterialScale are inferred from shader + data (not from EXE yet).
- Game material shaders: Homefront nodes PrimaryLightColor/Direction, MaterialUpper/LowerSkyColor
  (values measured in capture). Graph compiler gets custom-lit materials wrong (road was blue) → road
  uses the game's own shaders; general switch for all objects is opt-in (HOMEBOTS_GAME_SHADERS=1)
  until shadow/grass/decal constants are bound.

## Water / FluidSurface (2026-10-15)
- FluidSurfaceActor -> game shaders on FFluidVertexFactory (flat grid, GridSize = FluidWidth/Height). Overpass: MI_water_crossroads_lake_dynamic, translucent, unlit.
- SceneColorTexture: scenecopy.rs copies colour + depth*1000 (alpha) after the opaque pass; MinZ_MaxZRatio = (10000, 0).
- Shader map of a static-permutation MIC (verified, Overpass water trace 2026-10-15, D3D9, call 60498388):
  the game looks the shader map up by the instance's FStaticParameterSet (first set in the MIC native:
  BaseMaterialId + switches FName/Value/bOverride/GUID), matching base id + switch values against the
  cache entries' sets (after FriendlyName). The resource id in the MIC native points at a different,
  stale entry. One MaterialId can have several cache entries. With the right entry the MIC's full
  cooked uniform set matches the CTAB indices exactly (no filtering). Fallback (no match): id + filter.
- Water draw (trace): VS/PS = M_water_translucent entry 2265cd2f…, NoLightMap+SkyLight, translucent
  SRCALPHA/INVSRCALPHA, alpha ZERO/ONE, Z test on, no Z write. Uses PrimaryLightColor/Direction (sun
  specular), LightAttenuationTexture (screen shadows), real fog (NearFogParams 3500,1,70000,120000;
  FarFogParams 3500,25000), TextureCube_0 = 256² A8R8G8B8 render target (SceneCaptureCubeMapActor_4 in
  Overpass_sky, Location -8873,-9408,-24, LitNoShadows, Near 20, Far 1e6, fog on).
  FluidDetailNormal = 512² G16R16F render target (detail ripple simulation, DetailResolution 512,
  DetailSize 5000 → DetailCoordScale 28.8); at rest bound as 0. MinZ_MaxZRatio = (9.99, 0.001, ...);
  base pass writes alpha = MinZ.x / w + MinZ.y.
- "Splash rings" on the water (2026-10-15): not foam, not tiling. KFX_LevelDesign WaterCollisionPlane
  (StaticMeshActor_85, 32 units under the river surface, scale 115×80) has HiddenGame=true on its
  component and was rendered anyway: its texture showed through the water and its depth made the
  water count as 32 units deep everywhere (foam factor high). Fix: skip components with HiddenGame.
- WorldToLocal rows = columns of the inverse LocalToWorld (was rows; only matters for rotated objects).
- Still neutral: shadows (lit), fog (off), cube capture (white), detail ripple simulation (impacts).

## SpeedTrees (verified against the Overpass trace, 2026-10-15)
- USpeedTree native: parameter block ending with FBoxSphereBounds, then bulk arrays: 5 position
  streams (12 B) + 5 data streams (64/72/76/64/68 B) for branch/frond/leaf card/leaf mesh/billboard,
  u16 index buffer, then element groups (u32 count + raw 216-byte FMeshElement; FirstIndex +152,
  NumPrimitives +156, MinVertexIndex +160, MaxVertexIndex +164): branch 1, branch 2, frond, leaf card,
  leaf mesh, then one billboard element. PineTree ranges equal the trace draws (e.g. 0/173/0..105).
- Vertex declarations from the trace (stream 0 FLOAT3 position, stream 1 data): see
  speedtree_layout() in main.rs. Leaf mesh declaration not in the trace (grass not drawn there).
- VS constants: LocalToWorld = scale + translation only; RotationOnlyMatrix rows = columns of the
  actor rotation; CameraAlignMatrix rows = (normalize(Z x F), F x row0, F) with F = view direction;
  LODData = (leaf size lerp, alpha offset), 0 at LOD 0; TerrainHeight as for static meshes.
  PS: CheapAmbientColor = WorldInfo.CheapAmbientColor as (c/255)^2.2.
- Base pass = FDirectionalLightLightMapPolicy VS + ...SkyLight PS (trace). Cull none.
- Not yet: wind (all wind constants 0 = no sway; SpeedWind integrates WindTimes per frame), LOD and
  billboard switching (always LOD 0), leaf meshes (grass), component LOD/draw distances.

## Homefront per-LOD material overrides (2026-10-16)
- StaticMeshComponent.lodOverrideMaterialContainers[LOD].lodOverrideElementContainers[section]
  .pOverrideMaterial overrides section materials (Homefront addition). Overpass highway signs use it
  for the text variants M_HWoverPassSign02..04_C; the trace loads exactly those textures. LOD 0 used.
- 2026-10-16: the user's copy still had the old level.rs (deploy missed it) → sign stayed blank on the PC
  while local renders showed text. Lesson: after deploying, compare file sizes on the PC with the local tree.
  Overpass sign under the steel overpass = Overpass_oilspill StaticMeshActor_340, M_HWoverPassSign02_C
  (HWoverPassSign01_C is genuinely blank; full 512² mips live in Statics_FL2/STA_highway.upk).

## Dynamic grass (2026-10-16)
- Terrain.TerrainGrassLayers: per layer TerrainGrassBillboards (Texture, Min/MaxDrawDist, Near/FarWidth,
  Near/FarWidthDist, Height, Near/FarHeightDist, NumBillboardsPerCell, CameraAlignedY) and
  TerrainGrassMeshes. Cooked instances per TerrainComponent after the quad flags: u32 layer count, then
  per layer TArray of 8-byte instances (3 x half-float position via FUN_007e9a00, u16 with four signed
  3-bit slope terms * DAT_013ba394).
- Rendering: global shaders FDynamicGrassBillboardVertexShader/PixelShader; vertices built on the CPU each
  frame in view space (job FUN_004e6180, 6 verts x 48 B per billboard), draws in the trace at 60495358.
- Generator (FUN_004e6180, verified: 640/645 traced billboards to < 0.07 units, rest = jitter rounding):
  per billboard b of NumBillboardsPerCell N: jitter = table[set][N + (trunc(x)&7) + b] (runtime table
  DAT_017e8f50 + set*0xe0, writer not found; measured: set0 = 0, set1 = (+-45.25, +-45.25) diagonals,
  set2 = (+-64,0)/(0,+-64) period 4); z += slope terms (x<0: -x*s[12..15] else x*s[8..11];
  y<0: -y*s[4..7] else y*s[0..3]). iy = trunc(Py): scale s = 0.5 + (iy&7)/7, U mirror = bit 4 of iy.
  d = 2D distance jittered pos - camera. tw = clamp((d-NearWidthDist)/(FarWidthDist-NearWidthDist))*s,
  halfwidth = lerp(NearWidth, FarWidth, tw); th = clamp((FarHeightDist-d)/(FarHeightDist-NearHeightDist)),
  height = Height*s*th (view y, or world z if CameraAlignedY). U = 0.5 -+ ((1-tw)/2+0.5)*sign*0.5*hw/NearWidth,
  V = 0.1 top / 1.0 bottom; overlay UV = (pos - TerrainBoundsMin) / (Max - Min) (GrassColorTexture, 1 mip).
  Cull: every visible instance within MaxDrawDist is drawn (the game culls coarser, invisible extras only).
- PS (blob_call21233): texkill a < 0.33; colour = overlay blend of billboard and overlay (g >= 0.5:
  ov*L + sat(tex-0.5), else 2*tex*ov*L), L = light*(0.864,0.7385,0.5668)+(0.136,0.2615,0.4332);
  fade to SceneColor by sat(w/3000 - sat((|sceneZ-w|-50)*0.002)); opaque, depth write, cull none.
- Implemented (homebots grass.rs): static per-instance data + VS with the job's formulas, drawn after
  the scene copy. Open: wind (4 global sources at 0x1b93088, stride 0x14: x, y, r^2, phase, amp;
  writer not found; trace sway +-3 units). Shadows: now hf_sun_shadow (see below).
- Wind solved (2026-10-16): the sources are not written by address; wind manager object at 0x1b92f20
  (ctor 0x64aa50, 10 slots x 0x24: x, y, ?, lifetime, r^2, amp, speed, phase, short id). Per update
  FUN_00896060 advances phase += speed and FUN_00a5c490 sorts slots by camera weight and copies the
  best 4 (x, y, r^2, phase, amp) to +0x168 (= 0x1b93088), unused: r^2 1, amp 0; pointer stored in
  the grass manager +0x68. Level load FUN_00512420 adds one permanent source: (0,0), r^2 1e20, amp 3,
  speed pi*2*(1/62) per update. Job: top vertices view x += sum f*amp*sin(a+phase), f = max(r^2-d^2,0)/r^2,
  a = ((iy^ix)&7)*(1/7)*pi*2 (ix, iy = trunc jittered xy). Second output sat(sum f*sin)*0.7 goes to VS
  TexCoord1.z (cubed to TexCoord1.x), unused by the PS (input mask yzw). Viewer: phase per second
  (= game at 62 updates/s).

## Bus texture = wrong LOD (2026-10-16)
- Multi-LOD StaticMeshes: the map holds LOD1+ inline; LOD0 is separate bulk data (flags 0x11, LZO) in the
  source package. After the LOD headers come per-LOD bulk headers (flags, count, size, offset).
- Trace draw vertex counts = LOD0 → the game renders LOD0. lodOverrideMaterialContainers are indexed
  by the rendered LOD. Viewer drew LOD1 with LOD0's materials → bus UVs/texture looked wrong.
- Fix: read_static_mesh_full loads LOD0 from the source package; materials = lod_materials[lod].
  Verified: gas station 13 sections. Fallback env HOMEBOTS_NO_FULL_LOD.

## SpeedTree wind (EXE, 2026-10-16)
- SpeedTree 5 CWind: ctor 0xf84e90, SetParams 0xf85a60, SetDirection 0xf85b90, SetStrength 0xf85a80,
  Advance 0xf85250, gust 0xf85d60, smooth 0xf862f0 (logistic 12x-6), random 0xf861e0 = MT19937 seed 0,
  (u+0.5)/2^32. Wrapper 0xc51b50: dir (1,0,0), strength 0.5, dt clamped < 1 s.
- Shader table at +0xb00: WindDir, WindTimes, WindDistances, WindLeaves, FrondRipple, WindGust, GustHints.
- Native params: 38 floats ending 14 dwords before the stream arrays; mapping in upk speedtree.rs.
- Implemented: speedwind.rs (port), treewind.rs (per-frame write into the object buffer, OBJ_WIND).

## Sun shadows (trace projection PS blob_call20973, 2026-10-16)
- DirectionalLightComponent: FullScreenShadowCullDistance 800, FSShadowDistanceRatio 2.25, fade 30,
  filter radius 2.5 / 4 (full level). Cascades near [near,815], mid [785,1815] (1024²), full level
  fixed (1600², centre (-6417.23,-4959.99,4938.45), r 20128). Fit: sphere of slice corners, texel snap.
  4 PCF taps at ±R/(res·2√2), bias min(d·0.0001, 0.001). Implemented: sunshadow.rs (Bevy cascades
  overridden via reflection). Deviation: full-level map 1024² (Bevy: one size).

## Particles / fire (2026-10-16)
- Placed Emitters incl. *_FX sublevels; LOD0 sprite emitters; RawDistribution LUT [min,max,entries],
  entries = (len-2)/chunk; class defaults from Default__<Class> only when the struct is missing.
- Sprite VS (FParticleSubUVVertexFactory) PSA_Square / PSA_Velocity as in the cache shader.
- ParticleFogColor (Homefront node, PS 49ed7ca6…): lerp(Input, TranslucentFogColorTexture,
  sat((w-FarFog.x)/(FarFog.y-FarFog.x))); no fog in the viewer yet → Input (was black → black smoke).
- BLEND_Additive = One/One: GameMat outputs alpha 0 with Bevy's premultiplied Add (embers were black).
- Emitters whose material texture is missing are skipped (KFX_VHC smoke: MF_Smoke from KFX_Weapons,
  not in the local mirror; on the PC install it should load).
- DepthBiasedAlpha (PS 49ed7ca6… Bias = ZFeather -30; PS 3696ad02… M_firetest_alpha BiasScale 60,
  no Bias input → k = 1/30, so Bias default 0.5): sat((SceneW - PixelW) / max((1-Bias)*BiasScale, 0.001))
  * Alpha. GameMat now binds the scene copy (binding 21) for SceneW.
- Sideways fires (user, 2026-10-16): the engine's sprite CameraRight/CameraUp are NOT view right/up.
  Trace frame 3600 particle VS (blob_call15595): c12 CameraRight = -view up, c13 CameraUp = -view right
  (view rows from the same frame). With R = sin*CR - cos*CU, U = cos*CR + sin*CU, rotation 0 is upright.
- BLEND_Additive output (GRay_Test PS 11d676de…): rgb = Emissive * Opacity, a = 0 (white blocks were
  emissive without opacity). PixelDepth = ScreenPosition.w (view depth), not distance.
- God ray "Tic Tac" (user): KFX_Atmosphere.FX_Godrays.GRay_Beams_B = 6 square sprites (size 110, local
  offsets 0..100 along X) on an actor with DrawScale3D (5,1,1) and component InstanceParameters
  Color = (17,16,4,0). ParticleModuleColorByParameter Spawn (EXE 0x41f460, vtable 0x14f1990 slot 64):
  instance param matched by name only, channels * 0.0039077764 (0x13b0438) clamped [0,1]; fallback
  DefaultColor via 0xd50440: rgb = (c/255)^2.2, a = c/255; sets Color + BaseColor. Implemented, plus
  DrawScale*DrawScale3D on local particle offsets (LocalToWorld). Result: very faint glow streak.
  Verified by trace 2026-10-16 (god-ray trace, frame 2320, draws 57791132.., PS 11d676de = blob_call5964822):
  6 DrawIndexedPrimitiveUP quads, vertex = Position, OldPosition, Size, Rotation, Color(4f), UV (64 B);
  local positions 0..100, L2W row0 length 5 (DrawScale3D), vertex Size (550, 550) = 110 * 5 (square),
  Color (0.0664, 0.0625, 0.0156, 0) = instance colour * 0.0039078. Blend ONE/ONE, alpha ZERO/ONE,
  Z test LESSEQUAL, no Z write. Viewer now scales Size by DrawScale*DrawScale3D.
- Smoke missing on the PC too (2026-10-16): KFX_Weapons is a standalone package (Effects_FL2/KFX_Weapons.upk);
  inside it the package is the root ("Textures.MF_Smoke"), maps import "KFX_Weapons.Textures.MF_Smoke".
  Fix: Spawner::alias_package registers import packages' exports under "<pkg>.<path>" (also applies
  to env_reflections, fogdome, gcgamematerials). Smoke now renders with its texture.
- Open: mesh
  emitters (FakeLight) not drawn; simulation semantics UE3-standard, not trace-verified.


1. apitrace with TRACE_FILE: every process truncates the same file; the first capture only had the D3D9
   caps probe because the game renders with D3D11 (AllowD3D10=True).
2. Trace > 400 MB cannot be staged → split on the PC (split_trace.ps1), concatenate here.
3. Locally staged map set was incomplete (2026-10-15): Overpass has 14 sublevels + FL-Overpass.map.
   Always mirror the whole map folder before judging what's missing.
4. Trace setup 2026-10-15 (water, user request): apitrace 14.0 *win32* d3d9.dll (sha256 fdb77d7b…)
   in Binaries, AllowD3D10=False (backup GCEngine.ini.vor_trace), start via Documents\HomeBots\start_trace.bat
   (TRACE_FILE set, old trace/parts deleted, HOMEFRONT.exe started directly = one process).
   Afterwards: restore ini, remove d3d9.dll before any online play.
5b. Trace setup again (2026-10-16, user request, god ray): apitrace d3d9.dll was still in Binaries
   (sha256 fdb77d7b… verified). GCEngine.ini: backup GCEngine.ini.vor_trace2, AllowD3D10=True -> False.
   Afterwards: restore from GCEngine.ini.vor_trace2, remove d3d9.dll before any online play.
5. Rule (universal-modder): ask before changing game/graphics settings (the AllowD3D10 edit was made
   without asking; restored from backup).

## Phase 2: front-end menus (2026-10-16)
- No Scaleform: UE3 UIScenes in CookedPC/UI/*.upk, styles PC_Skins (UIStyle native StateDataMap,
  UIStyle_Text/Image/Combo, bEnabled=false state data falls back to Enabled), UE3 UFont, strings
  Coalesced_<lang>.bin. Layout = EXE FUN_00ebb610/FUN_00eab0c0 (see upk/src/ui.rs). Verified against
  trace frames 400 (main menu) and 600 (Mehrspieler) at 1366x768: every quad rect/uv/colour equal
  except code-driven text (version, changelist) and material tiles.
- Text (trace 400): line height = font's tallest glyph * scale (rows align with/without the tall
  space glyph), glyph y = top + VerticalOffset * scale, StylePadding (h, v) * autoscale applied for
  left/right and top/bottom alignment (Esc caption 6*1.28, version label 5*1.28).
- Image components: UIStyle_Image DefaultImage/Coordinates/StyleColor, UL/VL 0 = full texture
  (NavButtonBG VL 75/128 -> v 0..0.5859), ImageRef and StyleCustomization override.
- GCUIImage_ConsoleButton_* not drawn on PC (no 360_A quad in the trace).
- CookieTrail (trace 600): crumbs = CookieName of each scene on the stack, CookieStyle /
  PrimaryCookieStyle, bottom aligned; separator x = text end + CookieToSeparatorPadding*vw/1280,
  width CookieSeparatorWidth*vw/1280, vertical = (trail bottom - font height .. trail bottom)
  grown by SeparatorBorderExpansion, shifted by SeparatorVerticalOffset (unscaled). Exact match.
- Canvas PS (PS 20658): rgb = pow(max(sat(tex*vcolor), 1e-6), 1/2.2), alpha test > 0, blend
  SRCALPHA/INVSRCALPHA into the gamma-space back buffer. Viewer: hfpost now outputs gamma, the UI
  camera draws on top, uigamma pass converts to linear for the sRGB swap chain.
- Material tiles (MI_Gaussian_* blur, red strip): canvas material PS 2e5b4200 = emissive (material
  does its own pow 1/2.2), SceneTexture point-sampled from the scene colour *before* post
  (texture 0x2fabe540 is also the post input). matcompile now compiles SceneTexture; scenecopy
  has a second copy after the transparent pass for the UI.
- Version label text "VERSION: 1.5.500001" from trace 400 glyphs (format "%s: %d.%d.%d" in EXE).
- Navigation is data-driven: GCUILabel_MenuAction.m_MenuAction / CommandButton.GCCommand
  (UIMenuConfirmButton -> m_ActionCommand scene, UIOpenScene -> m_SceneName + m_LevelName,
  ExitToWindows). Offline whitelist: main menu, Mehrspieler, Spiel erstellen, options scenes;
  everything else drawn with GCUIInfo.m_LockedItemColor. Selected/unselected colours from
  GCUIInfos.GCUIInfo (m_SelectedItemColor / m_UnSelectedItemColor).
- 3D background: front-end camera = HFGameEntry CameraActor_0 (0,-2064,0) yaw 16384, matches
  CameraPosition c4 in trace 400-1200; projection x scale 3.405349 (y = x*aspect) from the trace
  (CameraActor FOVAngle 60 and the matinee FOV track 25 do not match it; source of 3.405 open).
  Levels: HFstreamMainMenu (U_MainMenuScreen.m_LevelName), HFstreamOnlineMenu (Mehrspieler
  command), post-process defaults from HFGameEntry WorldInfo (persistent level).
- Viewer: load_maps is re-runnable (LevelRequest resource; LevelEntity marker on everything a
  level spawns). Language: HOMEBOTS_LANG, else Steam appmanifest UserConfig language, else int.
- Fix (user report 2026-10-16): "Verlassen" did nothing: CommandButton.GCCommand is a single object
  property, not an array; UiWorld::objects accepts both.
- Main-menu background (user screenshot 2026-10-16: courthouse, not the crane): the game streams
  "HFstream" + campaign chapter map (GCUIInfo.m_SPMapInfos[i].MapName; EXE FUN_006a7900 "HFStream");
  user's game = chapter 1 Devil_P = HFstreamDevil_P (matches trace 400). HFstreamMainMenu
  (Defense_BG/FG, crane) is only the scene default. Offline build: chapter 1.
- Menu level switch took 1-2 s (user report): front-end levels now stay loaded (LevelGroup,
  hidden/shown), HFstreamOnlineMenu is preloaded hidden after the main menu; package index and
  shader cache are loaded once.
- Spiel erstellen (trace frame 1000 decoded with t2/tools/uitext.py: glyph uv -> font table):
  map lists = GCUIObject_MapListFrame drawn from its props (row height m_RowHeight*vh/720,
  9 rows even/odd colours, selected row m_RowSelectedImage 0.7 white, text m_FontScale*vh/720,
  x = column TextPadding*k (0.1-0.4 px off vs trace), line top + m_CellGridSpacing*k); list =
  the 8 maps the game shows for Bodenkontrolle + Overpass ("HIGHWAY", DLC2 thumb), only Overpass
  playable. Sliders: bar height BarSize*width/UL (3.39 px measured), marker MarkerWidth*vw,
  x = left + t*(w - marker). Check boxes CheckedImageComponent; combo box button = square at the
  right end (edit 199.0..412.5). Description label CLIP_Wrap: greedy word wrap. Loc keys prefer
  "<key>__pc" (PC text without the console "¶"). Defaults: slider CurrentValue (SpawnTime 5;
  the user's trace had 10 = saved setting), Auto. Team-Ausgleich + Karten wiederholen on (trace),
  VAC locked off, LAN selected (offline). "Starten" loads the first queued map's folder.
- Open: lock icon (m_LockIcon) for locked entries, selection bar in the Mehrspieler menu
  (trace 600 quad at the selected row), software cursor, Spiel erstellen logic, options logic.

- 2026-10-16 23:50: playable map switched from Overpass ("HIGHWAY") to Kreuzung = Maps/MP/Crossroads
  (the user's video + trace t3 are Kreuzung; user confirmed "es ist die selbe karte"). Overpass is TDM-only.

## Phase 3: match flow (2026-10-17, autonomous build, user checks the end result)
Sources: user's video 2026-10-16_23-57-47.mp4 (1280x720), trace t3 (hf.trace, 3231 frames, 1366x768).
- Launch -> "Spiel wird erstellt …" = U_MessagePopup (PC_UI_PopupMenus) over the create screen,
  iMessageLabel = ui_frontend GCUIScene_OnlineCreate CreatingMatchString (trace t3 frames 1026-1211,
  label x 440.7 = layout). Visible 3.0 s in the video (10 fps scan: 8.4-11.4 s), then the movie.
- Game-mode movie: DefaultEngine.ini GameModeTypes (FL -> GroundControl.bik, TDM -> battlePoints.bik).
  Decoder: crate infinitier_bik_decoder 0.0.14 (GPL-3.0-or-later, pure-Rust port of FFmpeg bink.c);
  Y planes of movie frames 0/94/288 equal the game's uploads byte for byte (SAD 0).
  Drawing (trace t3 frame 1216): Y 1280x720 + two 640x360 L8 planes, full-viewport quad, no blend,
  PS (CreatePixelShader call 20448): R = 1.16412354 Y + 1.59579468 Cr - 0.87065506,
  G = 1.16412354 Y - 0.813476563 Cr - 0.391448975 Cb + 0.529705048, B = 1.16412354 Y + 2.01782227 Cb - 1.08166885
  (tex1 = Cr, tex2 = Cb). Audio: binkaudio_dct 44.1 kHz stereo, decoded up front, played via bevy_audio.
- Subtitles: Movies/GroundControl.txt "start_ms,end_ms,key" -> Subtitles.deu [Subtitles] GC1..GC4,
  font PC_UI_Fonts.TradGothicBoldCondensed (DefaultLoadingSequence FontName), 4 black copies at
  (-1,-1),(-1,+1),(+1,+1),(+1,-1) then white (trace frames 1316/1425). Centre x = 682 at 1366
  (= trunc(0.1w) + (trunc(0.9w)-trunc(0.1w))/2), line start round(x - width/2), lines stacked up from the
  bottom edge with pitch 41 (= tallest glyph 38 + 3, measured). GC2 wraps between 537 and 614 px;
  limit = half the centred region (546) - exact rule not found in the EXE yet.
- Loading behind the movie: the trace shows the movie at frame 288 when the level was ready and the
  spawn selection appeared; the video shows the frozen movie frame during the final load. The loader
  here is synchronous, so the level load starts at movie frame 288 and the frozen frame (with subtitle)
  covers it; then the spawn selection opens.
- Spawn selection PC_U_SpawnSelectionMenu (trace t3 frame 1500 decoded, video 33 s):
  loadouts = GCGame.ini GCCustomLoadoutProfiles DefaultLoadouts (ANGRIFF, MP, SCHWER, SCHARFSCHÜTZE,
  TAKTISCH, SPRENGSTOFF), item names/images = Game/GCLoadoutItemInfos LoadoutItemInfos (ItemClassName,
  strItemDisplayName, ItemImage, InventoryType), abilities = AbilityInfos (strDisplayName, SmallIcon);
  costs = GCBattlePoints.ini SpecialAbilityCosts / VehicleCosts, InitialBattlePoints 500,
  vehicle rows "%d - KP" + gcgamecontent VehicleNameString (US vehicles sorted by cost, locked when
  > KP), auto spawn GCGame.ini PlayerSpawnManagerMP SpawnMenuAutoSpawnTime 30.
  GCUISimpleLabel/GCUISimpleImage objects (SimpleObjects): Position in 1280x720 space, label Size =
  text scale, HorizontalTextSpacing; title/headers match the trace to 0.05 px. UI materials with
  BLEND_Modulate/Additive now blend as such (MI_ColorMult_RedGrad panel). Console D-pad prompts hidden.
- Pause menu PC_U_PauseMenu_MP over the game (Esc); "SPIEL VERLASSEN" -> U_MessagePopup with
  UI_MenuText ExitGamePopUp ConfirmExit + Ja/Nein (Engine Menu YesString/NoString) as in the video;
  Ja returns to the main menu. The video's "Spielsitzung geschlossen." popup text is not in the deu loc
  files (not shown).
- Player (player.rs): Default__GCChar_Player cylinder R 15 / half height 41.5, BaseEyeHeight 35.5,
  crouch 32.5/22.5; GCCharacter GroundSpeed 260, AccelRate 450, JumpZ 375, SprintingPct 1.33,
  CrouchedPct 0.4, MaxStepHeight 26, CustomGravityScaling 1.15 x WorldInfo DefaultGravityZ -750;
  WalkableFloorZ 0.7; mouse 45 rotation units per count (GCPlayerInput DefaultMouseSensitivity).
  FOV from trace t3 frames 1960/2100: projection scales 1.1401/2.0278 -> vertical 52.5 deg.
  Spawn: team USA uses the "_High" PlayerStart groups (trace: first spawn camera at
  GameMode_PlayerStart_111, VehicleSpawnsHigh); infantry picks one of the 8 Infantry_High starts
  nearest the team's vehicle spawns (frontline logic still open). Collision = level triangles
  (static/skeletal mesh LOD 0 + terrain grids) in a 2 m grid, sphere push-out + floor ray.

- HUD PC_UI_PlayerHUD.PC_U_PlayerHudScreen (hud.rs): GC widgets draw SimpleObjects +
  CodeBasedSimpleObjects (both, the trace shows the duplicated labels twice); GCUISimpleLabel
  Justification, GCUISimpleLabel_Integer (m_nMaxDigitAllowed, padding digits at
  m_fDigitPaddingAlphaModifier 0.3: trace "0"+"30"). Values: timer = RoundTimeSlider minutes,
  team names UI_HUD GCUIObject_GameScore TeamUSA/TeamKOR ("KVA"), KP = InitialBattlePoints.
  Hidden: vehicle/drone/weapon placers and event widgets (warnings, crawl, MOTD...).
  Trace t3 frame 2700 (HUDFont_Green glyph table): timer x 67.2 base 494.2, KP right edge 1294.5.
  Open: score bars (GCUISimpleProgressBar), round squares, minimap, loadout-specific icons.
- Brush volumes (upk level.rs brush_volumes): UPolys = owner ref + FPoly (Base, Normal, TextureU,
  TextureV, Vertices, 36 bytes tail); Crossroads GameMode_FrontlineVolume_* are ~10 m prisms, each
  contains its Objective_GameMode_VolumeHold (3 sets of A/B/C: west x~-10000, middle x~0, east x~10000).
  Objective_GameMode_VolumeHold defaults: fCapTime 30 (map: 20), RegressionRate 2, RegressionDelay 2.

- Objectives (objectives.rs): Objective_GameMode_VolumeHold + FrontlineVolume brushes; three lines
  of A/B/C sorted along X, match starts on the middle line (assumption until the FL2GameFL script is
  decoded). Standing in the volume for fCapTime (20 s) captures for USA, +250 KP
  (PSR_CaptureObjective), HUD "BRAVO WIRD GESICHERT" / "BRAVO GESICHERT" (gcgame.deu
  Objective_GameMode LocalizedObjectiveStatus[1]/[5]) in GameObjectivesMP's code label. Leaving: after
  RegressionDelay 2 s the progress drops with RegressionRate 2 (per second; meaning of the unit not
  verified). Team score rate: not in config (script) -> scores stay 0 for now; trace t3 scores stayed 0.
- Leaving a match: ConfirmExit popup -> Ja -> UI_PopupMessages ClosingSession "Spielsitzung
  geschlossen." for 2.5 s (video 97.7-100.2 s) -> main menu. The game then shows its black loading
  screen (trace t3 frame 3130: "LADEVORGANG …" at x 458.4 / base 485.3 at 1366x768, scale vh/720,
  spinner Radial textures, 0.08*vw square, right edge 0.92*vw, bottom 707.4) - not built yet.

## Phase 4 (2026-10-17): first-person weapons, combat, bots, sound, effects

- Viewmodel (viewmodel.rs): weapons from WeaponConfigs MP.Infantry.CFG_* (weaponcfg.rs): SKMesh,
  MeshParts without bone (weapon body, rigid on the "Weapon" bone whose reference pose is the
  identity: bonecomp SKW_RIF_M4_1P Weapon q (0,0,0,1) t 0), the loadout's GCWeaponScope
  Mesh/MeshBase at ScopesRelativeLocation (0,-7.76,4.51), bHideFrontSight collapses the front_sight
  bone; CH_1P_AnimSet drives the skeleton, US_1P SKC_MP_US_1P arms follow by bone name.
  Bone convention: reference pose composed as stored (component = parent * local; bones within 1.3
  units of their vertices), animation track rotations are the conjugate (frame 0 vs ref pose,
  checked 1P and 3P). Scope variants: the weapon's Scopes table lists one variant per type; the
  loadout's variant (Holographic_var3) takes the table entry of its type and its own meshes.
  The sets have no hip idle (except AS_ATW_PZF3_1P_WP "IDLE"): the end of Draw is the hip pose.
- Bevy 0.16 note: the meshes are CPU-skinned every frame (positions relative to the eye, entity at
  the eye). On lavapipe a newly visible pipeline compiles asynchronously, the 1P meshes appear some
  seconds after spawning (cost a long debugging detour; not a bug of the code).
- ADS: RMB; FOVZoom of the scope entry (Holographic 50) as 4:3 horizontal FOV (vertical tan =
  tan(h/2)*0.75: matches the trace's hip 52.5 deg for 66.6 deg); 1P lateral scale follows the
  world/mesh tangent ratio with FOVMeshZoom (40); ADSBringUpTime 0.2 / BringDownTime 0.175;
  ADSMovementPercent 0.4, MovementSpeedModifier 0.95. Pose = blend of hip and ADS_idle.
- Firing: FireModes[0] FireInterval 0.095 (full auto), AmmoClip 30 / AmmoTotal 90, ReloadTime 1.587
  (reload_root_A), instant hit up to WeaponRange 3500; damage = MP.Damage.DamageGridMP (66 damage
  types x 22 physical materials, Int = near<<16 | far): M4 PMA_Soldier_Chest 35/25, Head 50/30,
  Limbs 30/20, linear from FullDamageRange 1000 to WeaponRange. Body regions on the cylinder:
  Default__GCCharacter HeadOffset 32 / HeadRadius 9; chest above centre-10, limbs below.
  NOT from data (marked): spread = Spread.Stand Min/Max/FireAdd/Decay x 0.1 as degrees, recoil =
  Recoil ViewPitch/Yaw x 0.1 deg per shot - units unverified (trace check open).
- HUD ammo: GCUISimpleLabel_Integer shows the reserve (video "0 90"); GCUIObject_AmmoCounter bullet
  bar: weapon BulletIcon (AmmoBar_SingleBullet 4x12.5) spread over CounterLength 117 from
  LowerRightBulletStartPosition, spent rounds at EmptyBulletIconAlpha 0.2.
- Health: Pawn 100, Default__GCCharacter HealthRegeneration 5 s delay, 10 HP/s. Death -> 4 s ->
  spawn selection again (KP and loadout kept).
- Frag grenade (G): GCThrowableWeaponInfo GrenadeFuseTime 3, PreThrowAnimationTime 0.56,
  ThrowAnimationTime 0.5, Ammo 2; GCProj_Grenade Speed 1500, TossZ 100, DamageRadius 400,
  m_fDampenFactor 0.2 / Parallel 0.45 (bounce); damage DamageGridMP EXP_Frag vs PMA_Soldier 200,
  linear falloff; sounds Default__GCProj_EXP_Frag_C ExplosionSound / GrenadeBounceSound; effect
  KFX_Pyrotechnics.FX_Grenades.Frag_Metal (the frag template GCGameContent imports).
- Special items (keys 4 / 5 = BindingsFoot EKBC_SpecialWeapon1/2 = loadout groups 4 / 5):
  CFG_EQP_FlakJacket 250 KP -> GCWeap_EQP_FlakJacket MaxHealthScale 1.5 (health 150);
  CFG_ATW_PZF3 250 KP, PurchaseAmount 3 (clip 1 + 2); GCProj_ATW_ATRL_C Speed 3500, DamageRadius
  250, DamageRadiusFull 50, straight flight (GCProj_Rocket m_fGravityFactor 0); stinger cues
  SC_Purchased_*. Key 1 back to the primary. Drones / UAV / bombs not implemented.
- Kills: KillStreak Objective_Mission_GameStreak Tiers (KillBP 20, KillGameScore 10..50 by streak
  0/5/8/11/15), PSR_Headshot +30 BP; team score += KillGameScore.
- Bots (bots.rs): MaxPlayersSlider-1 (16 -> 7 USA + 8 KVA), US_AST SKC_MP_US_AST / CH_AST
  SKC_MP_CH_AST with the loadout weapons' SKMesh3P on RightWeapon, CHC_MPAnimations sets
  (run_*_ads_us by movement direction, mp_sprint_F, stand_idle, crouch_idle, Death_1..10), 0.2 s
  cross-fades; mesh origin m_f3PMeshStandTranslation -44. Spawns: Infantry_High (USA) /
  Infantry_Low (KVA) nearest the team's vehicle spawns. Fire in bursts BotMaxFireRate 1 /
  BotMaxRefireRate 2. NOT from data: respawn 5 s, sight 6000 uu, aim error = Spread.Stand.Min/2,
  reaction 0.5 s.
- Navigation (nav.rs): the MP maps have no path network (Crossroads 16 ReachSpecs, no PathNodes),
  so a 1 m walk grid is built from the collision triangles in a background thread after loading
  (Crossroads: 512 x 260 columns, 149k nodes, 32 s on the test container), A* + LOS smoothing.
- Objectives with teams: presence of USA/KVA in the volume, contested = no progress
  (LocalizedObjectiveStatus[7] "WIRD UMKÄMPFT"), enemy-held -> neutral ([0] NEUTRALISIERT) -> own
  ([5] GESICHERT / [6] VOM FEIND GESICHERT). NOT from data: 1 team point per held objective per
  second (score rate is script code).
- Sound (upk sound.rs, sfx.rs): SoundCue graphs (Mixer, Random, Modulator, Delay, Attenuation; game
  nodes pass the first child), SoundNodeWave Ogg Vorbis from CompressedPCData (size 8 bytes before
  "OggS"), RawDistributionFloat LookupTable [min, max]. Weapon fire via WeaponConfigs SoundInfo ->
  A2_Weapons_C GCSoundInfoWeapon (SI_M4: Fire1P = SC_ACR_Fire1P_ForE3, Fire3P, ADSIn/Out); reload
  etc. via AnimSequence Notifies (AnimNotify_Sound, e.g. reload_root_A 0 / 0.996 / 1.501 s);
  MaxConcurrentPlayCount honoured (new plays skipped).
- Effects: pooled one-shot particle systems (particles.rs fire()): weapon EffectsInfo
  (KFX_WeaponNodes HandFired.KFX_RIF_M4) View1P/View3P/ViewADS Day MuzzleFlash
  (KFX_Weapons.HandFired.MF1P_M4_AM_A ...), frag explosion.
- Upk fixes: export_reader rejects a first tag with an FName number (US_AST: (bEnableShadowCasting,
  97) at offset 4); struct_prop / struct_array helpers; AnimSeq.sounds.

## Phase 4b (2026-10-17): vehicles, HUD markers, kill crawl, rounds, loading screen

- HUMVEE (vehicles.rs): GCV_US_M1114_Content, mesh SD_M1114 SKV_SD_M1114 (36 bones, CH_VARIANT
  bones collapsed), sockets from the mesh (bone frame X fwd / Y right / Z up): Seat0StartPosition,
  Seat0CharacterSocket, MGFireSocket, MGViewSocket. Wheels: SteerFactor front 1 / rear -0.85,
  WheelRadius 26, ride height 55.75; TorqueVSpeedCurve (-800,0.1) (-750,10) (0,80) (750,10)
  (800,0.1) -> top speed 750 uu/s; MaxSteerAngleCurve (0,35) (800,10). InterpCurveFloat raw layout:
  count, 24 bytes per point, 8 trailing bytes. Bought in the spawn selection for VehicleCosts
  BPCost (KP 500 -> 200 in the test). Keys from GCInput_deu.ini: E enter/exit (exit at
  Seat0StartPosition), F1 / F2 seats, 6 next seat, C behind view.
  Gunner: CFG_M1114_Seat1 FireInterval 0.05, overheat 4 / 0.12 / 1.5, spread 2,
  GCDmgType_M1114_Seat1_Bullet; sound A2_M1114_C.SC_M1114_50Cal_MP.
  NOT from data: kinematic driving (ACCEL 350, BRAKE 900, COAST 120 uu/s^2), 4-wheel bicycle
  steering, 4 ground probes for pitch / roll, wall spheres; 3P camera distance 400 (the data's
  min offset 250 lies inside the body). No vehicle damage, bots do not use vehicles.
- Objective markers: UI_PlayerHUD.Textures.Obj_Alpha / Obj_Bravo / Obj_Charlie, size Max 52 /
  Min 32, distance in m (50 uu = 1 m) in GamerTagFont at scale 0.6, alpha 0.5.
  NOT from data: the size mapping between Min and Max over distance.
- Kill crawl: GCUIObject KillCrawl at 0.0188 / 0.036 of 1280x720 with its TextScale /
  IconScale, killer [weapon UIIcon] victim, team colours from the GameScore label DrawColors.
- Score bars: GCUISimpleProgressBar (FilledFriendly / EnemyScorePills); a round is won by filling
  the bar (score limit = ScoreLimitSlider), two rounds win the match (Subtitles GC3 / GC4).
  Round win moves the front line, banner GCUIBannerMessageManager FrontlineMovedUp / Back, +100 KP
  (PSR_AdvanceFrontline); match end GCUIScene_WinLossSplash YourTeamWon / Lost, after 8 s back
  to the front end. NOT from data: the 8 s.
- Loading screen when leaving (menu.rs Phase::Loading): DefaultEngine.ini DefaultLoadingSequence
  (BackdropTexture EngineResources.Black, FontName PC_UI_Fonts.TradGothicBoldCondensed,
  HorizontalTextSpacing 2, SpinnerTextureName Radial2, SpinnerTextureName2 Radial1), text
  LoadingTips [Loading] LoadingText "LADEVORGANG …". Trace t3 frame 3130 (1366x768): scale vh/720,
  line top 425, left 429.75 (720-line units), advance = glyph width + 2, glyph top = line top +
  VerticalOffset (L 8, "…" 27); spinner square 0.08 vw centred at (0.88 vw, 0.85 vh), Radial2
  rotated clockwise under Radial1. NOT from data: rotation about 1 turn/s (video 100.3-101.3 s;
  the trace has no timestamps). Our level load blocks the frame, so the spinner stands still
  while loading; the screen is drawn 3 frames before the load is requested.

- Back in the front end after the loading screen: U_MainMenuScreen + U_OnlineMenuScreen
  ("HAUPTMENÜ > MEHRSPIELER", user's video 101.5 s) over HFstreamOnlineMenu; the main menu level
  is streamed in the background. (Before, the stack was empty after leaving.)
- Active objective (GameObjectivesMP): icon at ActiveObjectiveCenterPosition (640, 132),
  ActiveObjectiveSize 64, letter texture Obj_* tinted by owner (white while neutral, video 56-75 s);
  while it is being taken ProgressIconTemplate UI_PlayerHUD.Materials.M_roundedMask with its
  scalar parameter Progress = progress / fCapTime (grows counter-clockwise from 12 o'clock, as in
  the video; checked against a CPU evaluation of the compiled graph). The material compiler can
  now compile chosen scalar parameters as run-time values (MicChain.dynamic -> mat_param(i),
  UiMatGraph uniform 9). The world marker of the objective the player stands in is not drawn
  (video: only the active icon). "%s GESICHERT" stays while standing in an own objective
  (video 76-92 s).
- UI fixes: a batch entity that switches between UiMat and UiMatGraph is respawned (swapping the
  material component left the new material undrawn); the ring material is drawn as an empty
  quad outside the match so its pipeline exists before the first capture (a pipeline first
  needed in the match was not compiled for minutes on the software renderer).
- Minimap (minimap.rs): trace t3 frame 2700 renders a 256x256 target before the scene: clear
  black, orthographic top-down, projection scale 0.0004 -> 5000 uu across, then 60 draws:
  the *_minimap sublevel's HiddenGame plane (Minimap.Statics.Crossroads_minimap, MRM_Terrain,
  material MI_MiniMap_Crossroads = PS 0x617e4f60, our compiled graph), 54 MRM_Impassable
  objects (rocks, buildings; PS 0x2f8f9680: 0.45 * normalize(normal).z + UniformVector_0 = 0),
  4 draws PS 0x2f8fa580 (tex(vertex colour) * (0.694, 0.8, 0.652)) taken for MRM_Road with the
  lookup texture as 1 (NOT from data). Components carry MinimapRenderMode (upk
  minimap_placements); everything sits on RenderLayers 1 for a second camera that renders into
  the image the HUD's M_MiniMapTerrain samples (Textures.MinimapRenderTexture). Heading up
  (video: the arrow always points up), centred on the player. GCUIObject_MiniMap icons:
  PlayerIcon MiniMap_You 28, ObjectiveIcon MM_alpha.. 24 (clamped to the square, SquareMinimap),
  FriendlyInfantryIcon 12 for USA bots. The minimap texture Minimap.Textures.Crossroads_minimap
  streams its 512..32 mips from a package "Minimap" that is not in the staged copy (16x16 here).
  NOT from data: the capture centre follows the pawn feet (vehicle: the view), camera height.
  The green tint is M_MiniMap_ColorEffect (additive 0.097 0.15 0.097) and the scan lines are
  the MiniMapEffects placer's white 1-pixel images at alpha 0.1. The placer's stored Position
  (0.446 / 0.927 of the viewport) would put them off screen; trace frame 2700 draws them next to
  the map with an animated vertical offset that is not decoded -> drawn static at their own
  positions (WidgetRt.origin). Checked against the user's video (62 s): same road band, crater
  at B, rocks, tint and lines; the terrain image itself is the 16x16 mip here.
  Note: in trace t3 the HUD drew the map quad with a 1x1 texture (the capture was not shown in
  that session), so the HUD side is from the data + video.
- Score area (GameScore): the frontline parts are parked at y 1000..1040 in the data and placed
  by code; layout from trace t3 frame 2700: ProgressBarLine_2Boxes at (91, 464.8) / (91, 484.8),
  bars from x 107 (96 x 4 = Size 100 x 8 * BarShrinkPct 0.96 / 0.5), Token_Square round boxes at
  x 209 / 225 (alpha 0.75) with Token_Fill for rounds won. Matches the video.
- UI pipeline warm-up now covers every material the player HUD draws (not only the ring).
- Reward banner (GCUIObject_BannerMessage, GCUIBannerMessageManager BPGain "+%dKP" + the
  GCPointSpendingSystem LocalizedRewards name, e.g. PSR_CaptureObjective "Einsatzziel erobert"):
  iMessage1 at (640, 255) Size 0.68, iMessage2 at (640, 277.4) Size 0.48, HUDFont_Green, centred;
  MainBannerAnimation_Fast (iMessage1 scale 10 -> 0.68 and fade-in 0-0.2 s, iMessage2 fade-in
  0.2-0.5 s, fade-out 1.0-1.3 s) - video 77.2-78.3 s matches. Rewards queue one after another:
  capture 250, kills (PSR_KillAsInfantry, streak tier BP), headshot 30 (PSR_Headshot), round won
  100 (PSR_AdvanceFrontline). Round banners (FrontlineMovedUp/Back) use the same widget with
  MainBannerAnimation_Slow (fade-out 2.2-2.5 s). Scaling about the line centre is an assumption.
- Crawl events (GCCrawlMessage): FriendlyCapturedObjectiveMessage "Ihr Team hat erobert:" +
  LocalizedObjectiveName, EnemyCapturedObjectiveMessage, FrontlineMovedMessage, white, in the
  kill crawl. Video: the line's caps at y 131..141, x from 26, visible 77.2-81.7 s -> crawl lines
  live 4.5 s (kills too), newest at the bottom slot, top 125 (calibrated to the video, the box's
  vertical layout is code).
- KP count-up: the displayed TotalBattlePoints rise at ~167 KP/s (video 77-79 s: 500 -> 573 ->
  724 -> 750; NOT from data); spending is shown at once.
- Fonts: UFont Kerning is now added to every glyph advance (trace t3 frame 2700: HUDFont_Green
  Kerning -6, timer glyphs 16.9 wide advance 10.9 at native size; GamerTagFont -2). Menu fonts
  without kerning are unchanged (checked against the video's Mehrspieler menu).
- HUMVEE audio from A2_M1114_C.AudioSetup_M1114 (GCVehicleAudio, imported by GCGameContent):
  gunner SCFireSound SC_M1114_TurretGun_MP with bLooping (loops while the trigger is held; the
  earlier name-picked SC_M1114_50Cal_MP is gone), SCEngineIdle SC_M1114_Engine2 looping while
  occupied, SCEngineShutdown SC_M1114_Shutdown on exit. sfx.rs gained loops (SfxQueue::looping,
  stop when no longer requested). NOT modelled: rpm/sprint parameters (script), SCEngineSprint.

## Phase 4c: user test 2026-10-17 (video 15-16-27)
- Loading: every map package was opened (and LZO-decompressed) about six times per level load;
  now once, on all cores, and already in the background from the moment the match is created
  (main.rs preopen / take_preopened). The movie keeps playing until the packages are in memory,
  then the level is built (still one blocking frame, now ~15 s instead of ~43 s on the container).
- Auto spawn: the countdown used real time and the spawn menu was created in the frame that had
  just blocked on the level build (stale clock) -> the next frame saw 30 s gone. Countdown now
  advances by the frame delta clamped to 0.1 s, clicks are taken after 0.3 s on screen.
- Animation decoding: Homefront writes the 24-byte mins/ranges header before every multi-key
  rotation track, also for ACF_Float96NoW / ACF_Fixed48NoW (CHC_MPAnimations stand_idle: track
  sizes 24 + n*12, keys inside the header intervals). The header was read as key 0/1 -> one
  garbage key per loop = the bots' "back flips" (run cycles had 110-175 deg head swings, now
  < 50 deg; checked with examples/flipcheck + trackjump). Also fixes 1P sets in those formats.
- Weapon fire sound: Fire1P/Fire3P cues are GCSoundNodeSwitch "PlayFireRelease" graphs:
  [0] SoundNodeLooping (fire loop + one-shot punch/casing layers), [1] release tail. sfx.rs now
  keeps the loop running while shots come (per shooter, until FireInterval + 0.06 s without a
  shot, the 0.06 s is NOT from data) and then plays the release branch. Semi-auto cues without
  the switch (SC_M9_Fire1P, SC_M200_Fire1P) play once per shot as before.
- Distance: SoundNodeAttenuation was read from non-existent RadiusMin/RadiusMax (always 400/5000);
  now MinRadius/MaxRadius (RawDistributionFloat, defaults 400/5000), DistanceModel (Linear,
  Logarithmic, Inverse, LogReverse, NaturalSound with dBAttenuationAtMax; UE3 formulas),
  e.g. SC_M4_Fire3P layers 400..30000 (distant), 400..4000 (close). Positional loops follow the
  listener distance every frame. GCSoundNodeSwitch "Inside" takes child 0.
- Sound groups: [Engine.AudioDevice] SoundGroups of DefaultEngine.ini applied as the product of
  the group's parent chain (Weapons3P 1.5 x Weapons 1.2 x SFX 0.7 ...), divided by the common
  DistortionMaster x FL2SubMaster (0.3 x 0.4) that every in-game sound shares (= master level).
- Menu music: HFGameEntry WorldInfo AddlObjReferences SC_MainMenu / SC_MP_Lobby (GCUIScene_Menu
  natives PlayMainMenuMusic / PlayOnlineMenuMusic). Online scenes (class/name contains "Online")
  play SC_MP_Lobby, the rest SC_MainMenu; stopped outside the front end. NOT modelled:
  GCSoundNodeCrossFadeConcatenator (the lobby cue loops its first track).
- 1P sprint / walk: AS_*_1P_WP ADD_sprint_in/_loop/_out and ADD_move_F are additive (bIsAdditive,
  AdditiveRefName IDLE); applied UE3 style (rotation delta * base, translation += delta). In ->
  loop while sprinting, out after. ADD_move_F weight = ground speed / GroundSpeed, scaled by
  (1 - ADS) (the AT_CHC_PL_1P_Alpha GCAnimBlendAdditiveMove weighting is native, NOT from data).
- Quick throw: Default__GCChar_MultiPlayer_Content FragWeapSkelMeshComp = Quickthrows
  SKW_EXP_Frag_1P with ParentAnimComponent = the weapon mesh; the weapon comp's AnimSets add
  AS_CHC_PL_1P (cooked into the MP maps) with grenade_cook 0.43 s + grenade_throw 0.30 s. The
  weapon skeleton plays them, the rifle is hidden, the grenade shown until PreThrowAnimationTime
  0.56 (release), then the weapon's Draw. Merged by bone name (WeaponVm::merge_set).
- Projectiles visible: ProjectileMeshComponent meshes (GCProj_ATW_ATRL_C SKW_PRJ_ATMissile01,
  frag SKW_EXP_Frag_3P_static) follow rockets/grenades (6 pooled instances each); the rocket's
  ProjFlightTemplate (ST_ATW_Rocket) follows it (Particles::fire_tracked/track/release) and its
  AmbientSound loops at it. Projectile.MaxSpeed caps Speed.
- ADS: Scopes table OffsetLocationZoomed (1P mesh space, Z forward) now moves the weapon while
  aiming (ACOG z 12.5, Reflex z 5.0): before, ACOG/Reflex sat inside the near plane while aiming.
  Open: the holographic reticle (M_Crosshairs_HoloParent: ScreenAlign position minus the native
  "Offset" parameter, Scale 175) shows as a dot grid; the native code that sets Offset/Scale is
  not decoded (no holo draw in trace t3).
- 1P meshes (arms, weapon, scope, grenade) get a foreground flag in their object slot: their
  reverse-Z depth is squeezed into the nearest tenth (gamemat.rs), so walls / rocks right in front
  no longer cut the weapon (UE3 draws them in SDPG_Foreground with their own depth; the
  squeeze is our approximation).
- Terrain collision now uses the rendered quad diagonal (shorter height diagonal, see
  terrainemu.rs); the fixed diagonal put the feet up to half a quad's height difference off the
  drawn surface ("stuck in the ground").
- Bots (NOT from data; the MP game had no bots): objectives distributed by slot (every third bot
  guards a held objective, the others take the open ones by slot), goal points up to 5 m inside;
  under fire 65 % take the nearest nav node within 12 m hidden from the attacker (chest height
  1 m), crouch there 2.5-5 s; side steps while engaging close targets; reaction 0.6-1.4 s;
  deviation 1.2 + 4 x settle (1 -> 0.25 over 3 s of sight) + 1.5 while moving + Spread.Stand.Min/2
  degrees; no shot while a team mate is in the line of fire.
- Killstreaks: CFG_EQP_UAV (GCWeap_EQP_UAV MaxSweepDuration 30): enemies on the minimap
  (GCUIObject_MiniMap EnemyInfantryIcon). CFG_BMB_Hellfire: GCWeap_BMB_Hellfire_C ->
  GCV_AirstrikeUAV_C (AirSpeed 1000, fuel UCV_PredatorDrone VehicleSetup.Fuel MaxFuel 30) with
  MP.Vehicle.CFG_Airstrike_Hellfire (AmmoClip 2, ReloadTime 1.0, GCProj_Airstrike_Hellfire_Content
  Speed 10000 / MaxSpeed 7000, DamageRadius 600 / Full 200, ST_Hellfire, SC_HellFire_Fire_MP).
  NOT from data: UAV start 3000 uu above the player, pitch -55 deg (limits -85..-10), height kept,
  missiles fly straight at the crosshair point. Drones (CFG_DRO_GAssault / AAssault) still missing.

## Phase 4d: user test 2 (2026-10-17 evening)
- Movie: the level is no longer built while the movie plays. The packages are read in the
  background (at most cores-2 workers, two cores stay free for the movie / renderer); once they
  are in memory LoadingTips PCSkip "ZUM ÜBERSPRINGEN EINGABETASTE DRÜCKEN" shows (its place,
  centred at line 610 of 720, is NOT from data). Enter or the movie's end -> the black loading
  screen -> level build -> spawn selection. Tests (HOMEBOTS_MENU_LAUNCH) skip at frame 288.
- Holographic dot grid: imported textures took the address mode Wrap (the referencing package
  only has the import). Now the exporting package's AddressX/Y are used (world.rs texture()):
  M_Crosshairs_HoloParent's "center" sample is KFX_LensFX.Bloom01 (TA_Clamp), sampled at
  306 x (screen - 0.5): wrapped it tiled the lens with dots, clamped it is one centre dot.
  EXE check (Ghidra, FUN_00cae620): the only native crosshair parameter is "Offset" (FName 0xcea),
  set to (screen pos / viewport - 0.5) for bPaintedScope scopes and 0 otherwise -> holo reticle
  at the screen centre, as here. The fix applies to every material with imported textures.
- Aiming: the M4 ADS_idle pose puts the holo 6-11 units ahead of the eye (examples/posecmp:
  crosshair point at z 7.06), inside the 10-unit near plane -> the sight was clipped away. The
  1P foreground depth now also moves their near plane to 1 unit (gamemat.rs; NOT from data, UE3
  draws SDPG_Foreground with its own projection).
- Unscoped weapons use their own FOVMeshZoom (CFG_SNI_M110 5.0: the scope fills the view;
  before a fixed 40 left it far away).
- Additive 1P layers: in our conjugated convention the delta goes in front (delta * base);
  examples/addcheck: base * delta moved the left hand 4-6 units off the weapon while sprinting.
  Sprint state changes cross-fade 0.2 s, the walk weight follows the speed with 0.125 s (NOT
  from data).
- Road spline meshes are in the movement collision (CPU-deformed like the drawn road): the feet
  stood on the terrain below the road surface.
- Bots: view cone +-60 deg (all round within 5 m or 2 s after a hit), seen when head or chest
  is in the open; far goals get a detour point beside the direct line (side and distance by
  slot); sprint only on stretches > 25 m with no enemy for 3 s; goal points 1.5-6 m inside the
  objective (all NOT from data).
- Special weapon slots: ESpecialWeaponState (ESWS_ReadyToUse / ReadyToBuy / CantAfford /
  Disabled, GCGame.u enum); CantAfford is drawn grey (the native colour is NOT decoded).
  Keys: GCInput BindingsFoot four = EKBC_SpecialWeapon1, five = EKBC_SpecialWeapon2
  (loadout items 4 / 5).

## Phase 4e: user test 3 (2026-10-17, 19:02 list + original screenshots)
- Missing props: GCDestructibleStaticObject actors (BaseStaticMesh component, archetype merged:
  road signs DEST_*, trash cans; only the PersistentLevel ones, the rest are archetypes) and
  GCNudgeableCollectionActor NudgeableEntries (Location / Rotation / GCNudgeableInfo StaticMesh:
  tyres, crates, small signs) are placed now (Crossroads 2978 -> 3130 objects). Nudgeables have
  no movement collision (pushable physics props in the game).
- Sidearm: loadout group 2 (CFG_PST_M9) is a viewmodel weapon; key two = EKBC_Weapon2, mouse
  wheel = EKBC_NextWeapon / PrevWeapon (primary -> sidearm -> bought launcher), own ammo each.
- Bots "teleporting": the locomotion sequences carry root motion on the "Root" bone
  (run_front_ads_us 142.7 uu per cycle, mp_sprint_F 444 uu); the mesh ran ahead of the pawn and
  snapped back every cycle (also the hit cylinder lagged the drawn body up to 4 m while
  sprinting -> "I shoot and nothing happens"). Looped sequences keep the root in place (UE3
  RBA_Discard); the playback rate comes from that root motion per cycle vs. the ground speed.
- Bots stuck: nav edges need the floor to change by at most 27 uu per 25 cm along the edge
  (MaxStepHeight 26; kerbs of up to 55 cm passed before), plus a shin-high (32 cm) clearance ray;
  side steps are checked for walls / ledges; stuck > 1 s -> walk to a free node 4 m away first.
  A* uses reusable stamped arrays (was HashMaps) and at most two goal plans run per frame.
  (NOT from data: all of the bot behaviour.)
- Hip crosshair: GCUIObject_WeaponCrosshair of PC_U_PlayerHudScreen (Reticule_Tank_VerticalDash
  8x32 / HorizontalDash 32x8, CrosshairImageSizeLengthMultiplier 0.35, HUD green) when the
  weapon's Crosshair.UnZoomed.bCrosses is set. Dash inner end on the spread cone: half-angle
  spread / 2 deg through the vertical FOV - NOT from data, calibrated on the user's screenshots
  (M4 Stand.Min 4.75 -> ~30 px, M9 2.25 -> ~14 px of 720; the draw code is native). Shot
  deviation uses the same cone at the hip (bots already aimed with Min / 2), x 0.05 while aiming.
- Hit marker: HitNotifyMaterial M_Crosshair_HitNotify (TX_Reticule_Hit_v2) at HitEnemyImageSize
  64, fading over HitEnemyFadeTime 0.15, HitEnemySound A2_UI_C.SC_DamageProcured, on every hit on
  an enemy (bullets, rockets, grenades, drone).
- Tracers: GCWeaponEffectsInfo View1P / View3P Day Tracer (KFX_Tracers FX_50cal_1P / FX_50cal,
  every Frequency-th shot: M4 1P 2, 3P 3). The emitters' Size / Color / Velocity / Location are
  DistributionVectorParticleParameter (TracerSize, TracerColor, TracerSpeed, TracerOffset) set
  from the tracer struct; upk RawDist now carries parameter distributions (name + Constant).
  The tracer is removed at the impact distance (NOT from data).
- Rocket glow staying: ST_ATW_Rocket "Glow" (M_LightFlash) is one burst particle with Lifetime 0
  (lives with the emitter); releasing the trail at the impact now ends such particles.
- Vehicle sound: a looping cue played ALL its layers looped (engine start, revs, the gun's
  one-shot punch layers). Loops now loop only SoundNodeLooping layers; switch parameters can be
  set per loop (engine "Gas" from the throttle: rev / rev-down one-shots on change,
  "BypassStartup" 0 = start sound once), loop pitch from the cue's ModulatorValue range 1.0..1.2
  over speed 10..1000 uu/s (the value source is NOT decoded). The HUMVEE gun uses the weapon fire
  loop path (PlayFireRelease: loop while firing, release tail after).
- Gunner seat: crosshair at the gun's spread; HUD placers by mode: HUMVEE = VehiclePlacerCommon +
  VehiclePlacerJeep + Seat1Placer / Seat2Placer, the infantry placer hidden. The rifle no longer
  fires from inside the vehicle.
- Full map (M held, GCInput EKBC_OpenZoneMap StartFullMap / StopFullMap): GCUIObject_FullMap,
  M_FullMapTerrain = Textures.MinimapRenderTexture; the capture switches to the bounds of the
  ground GameMode_CombatVolume (Crossroads 48000 x 24000 uu; the MinimapVolumes span ~160000 uu,
  the level was a small patch), north-up (UE X right, Y down), at 1024x1024 (NOT from data);
  drawn inside 1260 x 440 at y 366 (user's screenshot), icons from the widget (MM_alpha.. 24,
  MiniMap_You 28 turned to the view, team mates). The red outer areas come from the level's
  minimap texture (only its 16x16 mip is in the staged copy here). The widget's Backing /
  FullMapEffects placers are hidden (their stored positions are off the map).
- Death / kill cam (PC_U_KillCamScreen): PlayerSpawnManagerMP ServerSpawnDelayTime 5 s, then
  the spawn selection. DelayCounter Message "ENTSTEHUNG IN:" (UI_InGameMenus Respawn
  SpawnDelayString, HUDFont_Green 0.72) and Counter, red (1, 0.048, 0.055) at 0.8, centred in their
  viewport boxes; GCUIObject_DeathMessage label (640, 500) GamerTagFont 0.72: GCDeathMessage
  Suicide / YouWereKilledBy + name. The player HUD is hidden meanwhile. Camera ~3 m behind, 2.5 m
  above the body, pulled in front of walls. NOT from data: the game's ragdoll is physics - the own body (SKC_MP_US_AST)
  plays one of Death_1..10 instead.
- Ground assault drone (drone.rs, CFG_DRO_GAssault -> GCV_SH_GAssaultWalker_Content): preview
  in front with "(LMT) ZUM ENTSENDEN EINER DROHNE" (UI_Keybinds LeftMouseButton,
  UI_GameplayHints DeployDrone), LMB sets it down and the view goes to it (pawn stays). Walking
  physics, GroundSpeed 275, camera at CameraOffset 50 above the cylinder centre, gun CFG_GAssault
  (0.15 s, spread 2, range 5500 / full 2500, overheat 3 / 0.3 / 1.5), battery: MaxFuel 1000,
  10/s moving, 1/s idle, 3 per shot; HUD VehiclePlacerAllDrones (frame, scales, "REMOTE DRONE
  FEED", DroneBatteryLife progress) + VehiclePlacerGroundAssault (reticle) from the data. E or an
  empty battery ends it. NOT from data: the drone turns with the view, the exit key, the feed's
  grey / scan-line post effect is not drawn, bots do not shoot the drone. Air drone
  (CFG_DRO_AAssault) still missing.
- Frame time: objectives and full-map bounds are read from the packages the level load already
  has open (they re-opened every map package on the first frame: seconds); sound packages are
  opened in a background thread at start (the first shot decompressed A2_Weapons_S in-frame);
  bots behind the camera keep their last skinned mesh, bots past 40 m are re-skinned every
  second frame. HB_PERF=1 logs per-system times every 5 s.
- Open / unverified: the rocket launcher's purple glare (its optic M_Scope_Lens01 + the ACOG
  reticle atlas MI_Crosshairs_ACOG_color1_fade1; the Homefront HDRLuminance node is still a stub)
  and the grey line in the scope were not reproduced here; trees have no collision.

## Phase 4f: user test 4 (2026-10-17 22:22 list, video 22-15-46)
- Missing cars (oilspill): 11 GCInteractiveSkelMeshActor whose SkeletalMeshComponent archetype is
  an import (VEH_Jiggle.Millitary.Destructibles.DSO_Buffalo_Destroyed.SkeletalMeshComponent0,
  exported by FL-Crossroads.map). Placements carry the import (`comp_import`), main.rs resolves it
  through the loaded packages and takes SkeletalMesh / materials from there. Buffalo_Destroyed's
  chunk list has a layout the parser runs past: when no vertex buffer follows, the search restarts
  after the index buffer with rigid (bone 0) skinning (509 / 509 skeletal meshes read).
- ESC in a round: the pause menu (PC_U_PauseMenu_MP) opened and the same key press reached it as
  "Back" in the same frame. Now consumed. Its server box is filled: ServerName
  GameReplicationInfo.ServerName (ini "HOMEFRONT Server"), game type, map
  ui_frontend.mapselect.<map>_name, friendly fire ServerInfo.FriendlyFireOn/Off.
- M: toggles the full map (open / close).
- HUMVEE: HUD mode (VehiclePlacerJeep/Common + seat placer) is set for driver and gunner;
  VehicleRepairing / MissileWarning hidden (no repair / lock-on offline). Sprint (Shift):
  MaxSprintSpeed 850, SprintTimeMax 2.25 s, SprintRechargeRate 0.5 of the class default,
  VehicleSpeedometer SprintMeter = sprint left, its BoostOn message only while sprinting; gun heat
  on GCUIObject_WeaponReloadOverheat FireModeProgress[0]. Gunner tracers: VehicleFireModes Tracer
  (FX_50cal, Size 10/150, TracerColor 18/7/1, Speed 7000, Offset -100, every shot) from
  MGFireSocket. Engine audio: Sprinting switch, SCEngineSprint; NOT from data: the tyre loop
  follows the speed, the JeepAccelerat / HumV_Gas loops the throttle (before every loop ran at
  full volume, also after stopping). Vehicle spawn waits for the pawn spawn (it could come at the
  old pawn position). No muzzle flash for the HUMVEE MG: none found in the data (FlashTags only).
- EMP blast: vehicles in EMPDamageRadius 600 are dead for EMPBlastDuration 4 s (SCEMPStart /
  SCEMPLoop of the vehicle audio).
- Tactical grenades on F (GCInput Grenade2), loadout item 2: CFG_EXP_EMP / _Flashbang / _C4 from
  WeaponConfigs (fuse, throw times, ammo) and the projectile default chain (Speed 1500 / C4 700,
  TossZ, DamageRadius / Full, bDetonateOnImpact for EMP, C4 sticks (no bBounce), sounds,
  ProjFlightTemplate). Meshes SKW_EXP_EMP_3P_static / SKW_EXP_C4_3P_static. C4 is set off by the
  next F (detonator key NOT from data). Explosion effects from the impact table (below; in the
  air PM_Default_Exp: FlashBang_Conc, EMP_Explosion_Impact, FX_C4_EXP). NOT from data: flash bang
  white-out / bots not shooting 1.5..5 s by distance, damage only where the damage grid has the
  type.
- Projectiles: the mesh pool was drawn at UE scale (rocket = giant black tube, grenade invisible
  around the camera): scaled to metres.
- Rocket trail: local-space emitters (ST_ATW_Rocket "Glow", Lifetime 100 s) end with the
  projectile; world-space smoke fades out.
- Reload: root sequence (reload_root_A/B/C, which one is NOT from data) + reload_branch_tact /
  reload_branch_dry (empty clip), the chain played at the rate that makes it last the weapon's
  ReloadAnimTime (M4 1.587 s, M9 1.6 s; at the sequences' own rates it took ~2.1 s). The M9 set has only the branches: it reloads visibly now. 0.2 s
  cross-fade into and out of the reload (NOT from data). Before only reload_root_A played and
  snapped to idle.
- Semi-automatic fire sound: each shot is its own trigger press (shot + release tail); fast M9
  shots kept one fire "loop" alive and went silent.
- Bots: a moving bot behind the camera is skinned again (its stale mesh jumped when the view
  turned: "teleport").
- Ragdoll (ragdoll.rs) for dead bots and the player's kill-cam body instead of Death_1..10. NOT
  from data: Verlet points on 17 main bones, fixed bone lengths, rigid torso, knee / elbow fold
  limits, ground contact with the level collision, start push 250 uu/s (bullets) / 600 + 350 up
  (explosions) away from the hit (PhysicsAsset / PhysX not modelled).
- Impact effects (impacts.rs): the level's PM_Library physical materials
  (PhysicalMaterialPropertyEx DamageTypeInstigators: CollisionEffect ParticleSystems / SoundCues
  per DamageTypes; the entry of the closest damage type class wins - class chains from GCGame.u
  super classes, e.g. GCDmgType_RIF_M4 -> AssaultRifle -> Hand_Bullet -> Bullet -; a material
  without an entry asks its Parent). Surfaces: each collision triangle carries its section
  material's PhysMaterial (Material / MaterialInstance parent chain), terrain the PhysMaterial of
  the layer with the highest weight at the hit (Crossroads: rock, grass, -, mud), soldiers
  PMA_Soldier_Chest / PMA_Head (blood Imp_Blood_Gen + SC_BulletImpact_Body_3P). Shots of the
  player, bots, HUMVEE gun and drone, and explosions (the material 1.5 m below, else
  PM_Default_Exp: frag on dirt Frag_Conc, in the air Frag_Air) - before, the frag always showed
  Frag_Metal and bullets had no impact at all. Some entries are empty in the data (M9 on dirt:
  decals only, not drawn).
- Footsteps: FootstepInstigators sound cue of the physical material under the feet (Parent
  chain; SC_FootStep_Dirt etc.), player 2D, bots within 25 m placed. NOT from data: one step per
  half locomotion cycle (root motion of run_front_ads_us / mp_sprint_F per cycle) instead of the
  sequences' footstep notifies.
- Collision flags of level meshes (StaticMeshComponent CollideActors / BlockActors /
  BlockNonZeroExtent / BlockZeroExtent, actor bCollideActors; missing = class default true):
  Crossroads has 537 meshes without collision (volumetric light cones, graffiti, stains, some
  beams) that were invisible walls for pawns and shots, 157 that only block pawns (fences, sign
  supports: shots pass) and 87 that only block shots (puddles, debris). Pawn movement, the nav
  grid and bot paths use the pawn set, shots / line of sight the trace set.
- HUMVEE VehicleOccupancy: the player's seat filled, gamer tag "Du" (as in the kill crawl, NOT
  from data), SeatLabel UI_HUD.VehicleHUD.DriverSeat / PassSeat, no voice icons.
- EMP explosion (EMP_Explosion_Impact) and the EMP / flash bang trail are mesh / trail emitters:
  not drawn yet.

- Video 22-15-46: the white disc at the frag explosion (72 s) is still open (smoke ring AFM_Shock /
  depth-biased alpha suspected); scope grey lines and 1P tracers (FX_50cal_1P Size 2 x 3 at
  10000 uu/s) wait for the original trace.

## Phase 4g: user test 5 (2026-10-18 11:50: stretched geometry, performance, DEAD ASSET, vehicle spawn, reload)
- Video 2026-10-17 22-15-46 is the original game (recorded with the trace, trace end 22:18:29 =
  video end): the white ground disc at 72.5 s is the original's own effect (second grenade), not a
  HomeBots bug. Trace staged and split (82 chunks of 50 frames, 4072 frames) for the comparisons.
- Stretched textures across the map (PC only, not with the container's software driver): Bevy 0.16
  GPU mesh preprocessing caches each entity's vertex / index slab offsets and refreshes them only
  when the entity's components change. Meshes rewritten every frame (particle sprites,
  CPU-skinned bots / vehicles / first-person weapon) are freed and reallocated in the slab on every
  change, so an entity that did not move drew with stale offsets = other meshes' vertices with its
  indices. `touch_modified_meshes` (Last) marks Mesh3d changed for entities whose mesh asset was
  modified, so they are re-extracted with the new offsets (bevy_pbr extract_meshes_for_gpu_building
  has no AssetChanged<Mesh3d> filter). Also the likely cause of the "broken" reload on the PC: the
  weapon entity keeps its transform while standing, its skinned mesh changes every frame.
- Performance / memory: DXT1/3/5 textures are uploaded as stored (BC1-3, the game's own mip chain,
  separate-file mips loaded per level) when the GPU has TEXTURE_COMPRESSION_BC (log "BC texture
  compression: true"; HB_NO_BC=1 for the old path) instead of decoding to RGBA8 and building mips
  on the CPU: 4-8x less texture memory / upload, faster loading. Container: peak RSS 2.75 GB
  instead of > 8 GB (OOM) with the full Statics_FL2 set.
- DEAD ASSET placeholders: the level list now follows the persistent map's LevelStreamingKismet
  objects: sublevels with bShouldBeLoadedAsPersistent are loaded, the others only when the game code
  streams them for a mode (TDM_*, *_FrontlinesData, *_bcsettings - names in GCGame.u / the EXE's
  package list). Crossroads_removedTrees (198 cut SpeedTrees and the M_DeadAsset_C placeholder
  meshes, never streamed) and the empty Crossroads_subpersistent are no longer loaded.
- Vehicle spawn: VehicleSpawnsHigh holds the helicopter starts too (CameraTransition
  ECT_AirVehicle_Normal, 15 of 36 on Crossroads, off the road); the HUMVEE uses the ground ones
  (starts with that transition get the group suffix "#air").
- Reload (AT_CHC_PL_1P_Alpha GCAnimBlendReload, children by GCAnimBlendReloadIndices: DRY 0,
  TACTICAL 1, RANDOM_ROOT 2..4): an empty clip plays root + reload_branch_tact (child
  "Reload-Dry"), rounds left root + reload_branch_dry ("Reload-Tactical"); before the branches were
  swapped. The roots end back at idle while the branches start mid-reload (pose distance curve,
  examples/seqjoin: the root is closest to the branch start ~0.2 s before its end): the root hands
  over RELOAD_BLEND = 0.2 s before its end with a 0.2 s cross-fade (blend time NOT from data); the
  chain is still scaled to ReloadAnimTime.
- Dev hooks: HB_SKIP=static,skel,road / HB_SKIPMESH=<path substrings> leave out level geometry;
  HB_NO_TOUCH disables the mesh re-extraction. New examples: meshbounds, skbounds, nearpl, streamlv,
  starts, seqcont, seqjoin, seqsounds, namerefs, enumdump.
- Found for later: the 1P weapon AnimSets carry AnimNotify_Footstep (footstep timing from data
  instead of the root-motion distance); GCAnimBlendReload / footstep natives located in the EXE
  (AGCCharacter execPlayNativeFootstepSounds at 0x56e1e0).

- Optimisation pass (2026-10-18 13:45, container CPU timings with HB_PERF, 2 cores shared with the
  software renderer):
  - Line checks (CollisionWorld::raycast_hit, bots' line of sight, shots, footsteps): 2D DDA over
    the grid cells in ray order with early exit, exact against brute force (unit test
    dda_matches_brute_force). Before: up to 4096 sampled cells deduplicated by linear search.
    bot_ai 15 ms -> 1 ms per frame.
  - Skinned parts (bots, ragdolls, vehicles, drone, first-person weapon) find their entities through
    a mesh -> entity index (MeshEntities) instead of scanning every mesh entity of the level per
    part and frame; the same index drives the re-extraction of modified meshes.
  - CPU-skinned section meshes hold only the vertices of their section (before every section mesh
    carried and re-uploaded all vertices of the mesh: 2-4 copies for bots, up to 10 for weapons).
  - Bot skinning: one matrix per bone (world rotation x axis swap x skin rotation), vertices in
    parallel chunks on the compute task pool. render_bots 10-12 ms -> 5-8 ms here (more cores on
    the PC).
  - Match start: objectives / full-map bounds reused from the level loader (the comparison failed
    because the match level list leaves out TDM_*; each re-read all map packages: 3-5 s freeze when
    the round started); level_files cached per folder and reads only the persistent map.

- User test 6 (2026-10-18 15:07):
  - Thrown grenades / rockets / C4 had no model: the projectile mesh pool was empty - the
    standalone Quickthrows.upk / Projectiles.upk copies of the StaticMeshes do not read (their
    data is cooked into the maps); now the copies in the loaded maps are used (log "projectile
    mesh ...: n sections"). Verified in slow motion (HB_TIMESCALE=0.25, HB_PROJDBG=1).
  - Grenade keys: G = tactical, F = frag (user: the original has it this way; GCInput binds G ->
    Grenade1 "GamepadThrow 0", F -> Grenade2 "GamepadThrow 1", which throwable index 0 / 1 is
    was not traced).
  - Footsteps: GCSoundNodeSwitch without ParameterName uses the class default "BehindView"
    (GCGame.u Default__GCSoundNodeSwitch); "Multiplayer" = 1 in a match; BehindView = 0 for the
    player's own (2D) sounds, 1 for sounds placed in the world. Before every switch took branch 0:
    the bots' steps used the local-player branch without attenuation (heard at full volume up to
    25 m, i.e. constantly) and the SP variants. The player's steps now follow the
    AnimNotify_Footstep times of the 1P walk / sprint layers (ADD_move_F 0.01 / 0.43 of 0.83 s,
    ADD_sprint_loop 0.01 / 0.35 of 0.7 s) instead of a distance (that gave ~6 steps/s), at 0.35
    volume (NOT from data, user request "much too loud").
  - Reload checked frame by frame in slow motion (HB_TEST_RELOAD=<s>[,dry], HB_VMDBG VMANIM):
    root (mag swap, the mag bone moves) -> branch with cross-fade -> idle, 1.587 s for the M4.
  - Dev: HB_TIMESCALE (game time speed), HB_TEST_RELOAD, HB_PROJDBG.

- User test 7 (2026-10-18 16:34 video 16-30-34, 16:44 original screenshots, 17:18 crash.log):
  - Reload still frozen on all guns: the chain sampled every sequence with hold_end = true, i.e.
    always its last frame (one still pose for the whole reload). Now sampled at the reload time.
  - Particle pool build 26 s -> 3.5 s (container): the emitter templates are read in parallel
    (std::thread::scope, chunks per core), each pool instance clones its template.
  - Grey cross in all sights: KFX_LensFX Bloom01 / Bloom02 (the masks of the sight crosshair
    materials, M_Crosshairs_HoloParent etc.) and holoHair have no mip chain in the game
    (NumMips 1, CompressionNoMipmaps), but HomeBots built one on the CPU for every RGBA/G8
    texture. The crosshairs sample them in screen space at ~6-60 texels per pixel, so a small
    blurred mip was read; its edge texels are not 0, and with TA_Clamp they extend along both
    axes (mask x250 -> full) -> lines through the dot across the whole lens. Textures without
    mips in the data now get none (gamemat::load_texture). Found by leaving out materials one by
    one (dev hook HB_SKIPMAT=<path substrings>) and a CPU evaluation of the crosshair shader.
    Also fixed on the way: the game-shader path (translated SM3 shaders) gave imported textures
    TA_Wrap samplers (an import has no AddressX/Y in the referencing package); now the address
    modes of the export the import resolves to (World::tex_addr).
  - Crash (panic in bevy_ecs::apply_deferred, crash.log): an automatic-fire sound loop started
    and stopped within one frame at a respawn; the entity was despawned before bevy_audio's
    play_queued_audio_system inserted its AudioSink -> command on a missing entity -> panic.
    Stopped sound entities are now despawned once their sink exists (paused and muted until
    then; despawned after 10 s if the source never decodes). Also GLOBAL_ERROR_HANDLER = warn
    (bevy feature configurable_error_handler): a failed entity command is logged, the match
    goes on.
  - C4 (user's screenshots of the original: charge on the ground, detonator in hand,
    "(LMT) ZUM DETONIEREN VON C4"):
    - CFG_EXP_C4 C4DetonatatorWeapon = CFG_EXP_C4Detonator (GCInfantryWeaponInfo: SKW_EXP_C4det_1P,
      AS_EXP_C4det_1P_WP with Draw / Fire1 / IDLE, EquipTime 0.8, bDisableZoom) is loaded as a
      weapon of its own; after the C4 throw (ThrowAnimationTime) it is drawn.
    - Left button with the detonator: all placed charges go off, Fire1 plays; afterwards the
      weapon held before comes back (NOT from data: the switch back was not traced). Without a
      charge out the detonator is put away too.
    - Hint: UI_GameplayHints C4Detonate with UI_Keybinds LeftMouseButton "(LMT)", drawn like the
      drone hint while the detonator is out and a charge lies.
    - G throws further charges while any are left (UI_Armory C4_Text: several can be placed and
      set off together); before, G set off the placed one.
    - Holding R sets off all charges (LoadingTips MPTips Message31); the hold time 0.5 s is NOT
      from data.
    - The C4 throw plays CFG_EXP_C4's CH_1P_AnimSet AS_EXP_C4_1P_CH (its own grenade_cook /
      grenade_throw) instead of the frag throw, without the frag in hand (the data has no 1P C4
      mesh; what the original shows in the hand during the throw is NOT verified).
    - Dev: HB_TEST_DETONATE=<s> (press the detonator once that many seconds after spawning).
  - Sights while zooming in: world FOV and 1P mesh FOV now change in their own windows of the
    zoom as the weapon / scope data says (WorldFOVStartTime / EndTime, ADSBlendStartTime /
    EndTime; M110 world 0.3-0.4 s, mesh 0.01-0.4 s with FOVMesh 50 -> FOVMeshZoom 5), the 1P
    lateral scale follows from both (viewmodel::lateral_scale). Before both changed together
    with the ADS blend, so the sight wandered over the screen during the zoom. Linear in the
    angle inside each window: NOT from data.
  - M110 at full zoom (container, 1366x768): the reticle crossing sits 6 px above the screen
    centre, where the shots go (camera and shot use the same eye / direction). The user saw the
    hits above the cross; NOT resolved - needs an original screenshot of the M110 zoomed in.
  - Holo red ring around the dot: OuterRing / MiddleRing 0.5 are also set in the original
    (trace chunk 51, PS c20 / c21 = 0.5 with the dot colour 6.666, 0.25, 0.25 in c13 / c18; the
    "Offset" parameters stay 0, c11 / c12 are the stipple panners). Kept as in the data.


- User test 8 (2026-10-18 18:34):
  - Round change: the spawns now move with the front. <map>_FrontlinesData has one
    GameMode_FocusedFrontline per line with its Objectives and per team a SpawnPointCollection
    (SpawnPoints by Type GMST_Infantry / GMST_Jeep / GMST_TreadedVehicle / helicopters, each a
    list of GameMode_PlayerStarts; the team from the starts' Group *_High = USA, *_Low = KVA).
    Player (infantry, or the jeep points with a HUMVEE) and bots spawn at the active line's
    points (objectives::line_spawns); without frontline data the old base-side choice stays.
  - Bots on a new round: every bot drops its goal and plans for the new line at once (before up
    to 6 s on the old goal, and spawning at the old base).
  - Bot AI (NOT from data, the MP game had no bots):
    - with all objectives of the line held, the attackers hunt the nearest enemy instead of all
      standing in the held areas (every third bot still guards);
    - reaching the goal plans the next point within 1-3 s (was 4-6 s), so capturers / guards keep
      moving inside the area;
    - idle watchdog: free to move but standing for 6 s -> new goal, the next objective (bias);
    - paths: the goal node is taken in the bot's own nav component (nearest one to the goal when
      the random node lies elsewhere); a bot on a small component (< 200 nodes: roofs, car tops,
      ledges) first steps onto the main walkable area. Before both cases gave no path and the bot
      ran a straight line into a wall. A* limit 60k -> 150k expansions.
  - UAV: enemies on the minimap (and now the full map) in red: GCUIObject_MiniMap
    EnemyBlipMaterial PC_Textures.EnemyBlip, VectorParameter "Color" (1, 0, 0, 1); team mates stay
    white.
  - Minimap froze (likely after the full map, M): the capture texture is resized 256 <-> 1024,
    which gives it a new GPU texture, but Bevy keeps a material's bind group until the material
    changes, so the HUD kept showing the old texture. UI materials using a modified image are now
    marked changed (menu::refresh_ui_mats). NOT verified on the user's case (cause unknown to
    the user).
  - Debug menu (F1, debug.rs, egui via bevy_egui 0.36 on the HUD camera; the game keeps running,
    input off, cursor free):
    - Spieler: +1000 / +5000 KP, KP 0; god mode, infinite ammo (combat::Cheats); refill health /
      ammo / grenades; start UAV; teleport to A / B / C of the active line; kill self.
    - Bots: kill all enemies / team mates / bots; freeze AI; bots hold fire; table per bot (HP,
      goal and distance, seconds standing, resets, nav path ok, fighting, teleport there),
      filter "only problems".
    - Match: score / rounds / time; win or lose the round (front moves); set the owner of the
      active line's objectives; game speed 0.1-4x.
    - Bot-Log: record bot events (goals, stuck, idle resets, respawns, front changes), write to
      logs/bots_<unix time>.log with a snapshot of every bot every 2 s.
    - Test hooks: HB_DEBUGMENU=<player|bots|match|log>, HB_BOTLOG=1, HB_TEST_ROUND=<s>.


- User test 9 (2026-10-18 19:57, screenshots):
  - Freeze after the match was won: the end of the match went straight to the front end
    (Action::LeaveDone), so the front-end level loaded inside the match's last frame with its
    sound loops still running (and the HUD scene kept). Now the same way out as "leave match":
    ClosingSession popup, black loading screen, front end. In the container the old path came
    back to the menu after a long load (no hang found); the new one stops the match first.
  - Pistol sound on fast clicks: SC_M9_Fire1P has MaxConcurrentPlayCount 2 and no release
    branch; a third shot while two still rang was dropped (silent shots). The game plays a
    weapon's fire cue on the weapon's AudioComponent, which restarts the cue; weapon shots now
    cut the oldest instance at the limit instead of being dropped (other cues keep the drop).
  - Shots beyond 70 m (35 m M9) never hit: the trace was as long as WeaponRange, which is the
    damage fall-off distance (DamageGrid: far damage beyond it). The trace is now 16384 uu
    (164 m; ZoomSnapToEnemyMaxDist 16384 on every weapon; NOT from data as the trace length).
  - Iron sights under a scope: the weapon's Scopes entry bHideRearSightBone (M249, ACR: true)
    now hides the rear_sight bone like bHideFrontSight the front one; the M249's ScopeRailMesh
    (SKW_ACH_M249_Rail) is drawn with a scope. A scope entry's ADSBlendStartTime / EndTime of
    0 / 0 (CFG_LMG_SCARMG) now means the weapon's times.
  - Bots stuck in geometry (container entrance, truck under the bridge) with the debug menu
    showing 0 s standing: the per-frame speed counted pushing / sliding against a wall as
    moving. New stall watchdog (NOT from data): wanting to go somewhere (goal > 2 m, not in cover)
    but staying within 1.5 m of one spot for 4 s (8 s while fighting), or not 2 m closer to the
    objective in 15 s -> a free node 3-8 m away, then a new goal (next objective); the third
    stall in a row puts the bot on the nearest node of the walkable map. The debug column is now
    "Hängt" (seconds stalled).
  - Paths to goals another nav component holds: the bot's component's node closest to the goal
    anywhere on the map (was: within 30 m, else a straight line). The nav log lists the largest
    components.
  - HUMVEE stuck on slopes / in ditches: the four bumper spheres (20 uu above the ground) touched
    rising ground ahead of the car and stopped it like a wall from about 10 deg on. Contacts whose
    normals all point up (>= 0.6, slopes to ~53 deg) are left to the wheel probes now (threshold
    NOT from data).


- Match load time (user 2026-10-18 20:35: the original's loading screen takes 21 s; the user's
  log: popup 3 s + movie 21.4 s + black loading screen 25.6 s):
  - The level is built while the "Spiel wird erstellt" popup shows (the build blocks the frame,
    the popup stays), then the movie plays with the level ready - skippable at once with Enter
    (LoadingTips PCSkip) - and the spawn selection follows; no loading screen after the movie.
    The 3D view is off while the movie covers the screen.
  - Texture mips stored in other package files (7149 for Crossroads, 150 MB read + LZO) are read
    on worker threads during the popup together with the level packages
    (upk::library::Library::prefetch); the build takes them from memory.
  - Package files for those reads stay open (one handle per file, positional reads) instead of
    one open per mip (slow on Windows with a virus scanner).
  - Particle templates: the class default objects are found through an index (a linear search
    over GCGame's 64k exports per particle module): fx pool reads 3.6 s -> 0.02 s.
  - Generated WGSL that validated once is not validated again (same source in many materials;
    the results are kept per build of the program in the temp folder): 1.3 s -> 0.01 s.
  - Container: level build 10.5 s -> 3.6 s. Each load logs "load phase: ..." with times and
    bulk reads, HB_PERF=1 adds totals of the build steps (ld_material, ld_texture, ...).


- Crash 2026-10-18 21:27 (exit 0xc0000409 STATUS_STACK_BUFFER_OVERRUN = Rust abort on Windows:
  stack overflow, panic while panicking or a panic across FFI; no log): the cause is not known
  yet.
  - The log now also goes to logs/homebots_latest.log (previous run: homebots_prev.log),
    unbuffered; a panic writes its thread, message and backtrace there.
  - The main thread gets a 32 MB stack on Windows (build.rs, linker /STACK; Windows default
    1 MB, Linux 8 MB): the level build runs on the main thread. In the container a 1 MB main
    stack (ulimit -s 1024) did not overflow during the load, so this is a precaution.

## Open
- Headless lavapipe renders some far views black (driver issue; user's AMD GPU fine) → verify on PC.
- Missing in viewer vs game screenshots (2026-10-15): road with markings, vehicles, trees (SpeedTree),
  grass foliage, terrain detail/colour; overall image too grey.

## Crash 0xc0000409 im "Spiel wird erstellt"-Popup (2026-10-18)
- Ursache (aus Daten, `examples/prefcheck.rs` auf Crossroads): der Match-Preload las alle Texture2D-Mips mit
  BULK_SEPARATE_FILE, auch ungenutzte Mips (Flags 0x21 = SEPARATE|UNUSED, Offset/Größe 0xFFFFFFFF).
  `vec![0u8; 4 GB]` je Worker -> Allokationsfehler -> Rust-Abort (kein Panic, daher nichts im Log).
  Linux überlebt das (Overcommit), Windows nicht.
- Fix: BULK_UNUSED wird übersprungen; read_bulk liest nie über das Dateiende; decompress_chunk_stream
  lehnt kaputte Header ab (>1 GB, Blockgröße 0); Preopen-Thread 64 MB, Prefetch-Worker 16 MB Stack.
- Ergebnis Test 10 (PC): kein Crash mehr; Match-Laden 6,1 s (Level 4,65 s), Preload 2,16 s parallel zum Popup.

## Konsolen-Spam / Lautstärke / Bot-Log (2026-10-18)
- WARN "despawn ... entity does not exist": sfx stoppte Sounds UND despawnte sie; bevy_audio despawnt
  PlaybackMode::Despawn-Entities nach dem Stoppen selbst -> doppeltes Despawn. Jetzt nur noch stop().
- GlobalVolume 0.8 (NOT from data: Wunsch des Users, alle Sounds -20 %).
- Bot-Log: Ereignisse werden ab Matchstart im Speicher gehalten und beim Start der Log-Datei mit
  hineingeschrieben; jede Zeile wird sofort geflusht. Der Log des Users enthielt nur 2 s (1 Snapshot).

## Profiler, Rig-Debug, Ton-Verzerrung (2026-10-18)
- Bevy-Features `trace` + `sysinfo_plugin`. perf.rs: Tracing-Layer misst jedes System, jeden Schedule,
  Render-Graph-Knoten und die Render-Sub-App (lock-freie Slots); RenderDiagnosticsPlugin liefert
  GPU-Zeit + Pipeline-Statistik pro Pass (eigene Pässe hf_* / scene copy mit pass_span).
  F2 Overlay, Shift+F2 Aufnahme -> logs/perf_<unix>.jsonl (pro Frame) + _summary.txt
  (Perzentile pro System, 30 schlechteste Frames mit Ursachen, GPU-Pässe, Zähler, Adapter/Settings).
  HB_PERF=1: Overlay + Aufnahme ab Matchstart (HB_PERF_REC=<s>, Std. 180). HB_VSYNC=0: ohne VSync.
  Zähler: skinned_vertices_uploaded, bot_parts_skinned, audio_voices, audio_voices_culled.
- rigdebug.rs: F3 Skelett-Overlay + Bone-Tabelle + Animationszustand (1P / Bot / Fadenkreuz-Bot, F4),
  Anim-Tempo/Einfrieren, Snapshot (Shift+F3) logs/rig_<ziel>_<unix>.txt, Frame-Aufnahme
  logs/rig_<unix>_frames.csv, Rig-Prüfung logs/rig_check.txt. HB_RIGDBG=1p|aim|bot für Tests.
  F1-Menü: neuer Tab "Werkzeuge".
- Erste Rig-Prüfung (Container): 1P-Arme haben 2 Bones ohne Treiber (RightWeapon, LeftWeapon);
  Bot-Rigs: Helmet (+ HeadExtra1/2 bei Rig 1) ohne Track in allen 3P-Sets.
- Ton kratzig nach einigen Minuten: Kanal-Limit [XAudio2.XAudio2Device] MaxChannels=64
  (BaseEngine.ini / GCEngine.ini). Darüber werden die leisesten One-Shots zuerst gestoppt, dann die
  ältesten (Auswahl NOT from data, UE3 nimmt Priorität*Lautstärke). Gestoppte Loops werden wieder
  despawnt (bevy_audio despawnt nur PlaybackMode::Despawn selbst).

## Test 10 (Profiler-Log des Users), Drohne, Bot-Wege (2026-10-19)
- PC-Profil (AMD Radeon iGPU, Vulkan, 1280x720): 27,5 FPS, Frame 36,4 ms. Render-Thread 32,6 ms ist der
  Engpass (render_system 17,2 ms, allocate_and_free_meshes 8,9 ms bei 404 geänderten Mesh-Assets pro Frame),
  GPU-Pässe nur 14 ms. Main: render_bots 5,1, particles 4,3, play_queued_audio_system 2,4 (p95 11,7) ms.
- Partikel: leere Emitter schrieben jedes Frame ein leeres Mesh (Upload + Draw im Transparent-Pass).
  Jetzt einmal leer geschrieben und versteckt, wieder sichtbar sobald Partikel da sind.
- Audio: Ogg wird beim ersten Laden einer Wave einmal zu PCM/WAV dekodiert (lewton, wie rodio);
  bevy_audio parste vorher für jeden gestarteten Sound den Vorbis-Header im Main-Thread, und der
  Audio-Thread dekodierte jede Stimme live.
- Bot-Skinning: unter 15 m jedes Frame, bis 40 m jedes 2., darüber jedes 3. (NOT from data).
- RenderDiagnosticsPlugin (GPU-Timestamps / Pipeline-Statistik) nur noch mit HB_PERF=1 oder HB_GPUDIAG=1.
- PC-Freeze nach dem Beenden: im Log nichts (letzte Zeile = Hauptmenü geladen). Verdacht: Treiber.
- Drohne: AnimTree AT_UCV_GAssault_Walking (SVehicleMesh AnimTreeTemplate) umgesetzt:
  GCSkelControl_TurretConstrained (Turret_Pitch folgt dem View-Pitch, Turret_Rotate yaw 0 da der Körper
  mit der Sicht dreht), GCSkelControl_LookAtVelocity auf TREADS_BASE (RestVelocity 10), Rad-Rollen
  WHL_L0..2 / WHL_R0..2 über GCAnimNodeBlendSingleBones -> GCAnimNodeVehicleWheel_0/1 (AXIS_Y, invert,
  WheelRadius 3, RollFactor 1.5), Antennen-Feder GCSkelControl_Spring (80 / 5 / max 10).
  NOT from data: Ketten drehen max. 360°/s, zeigen beim Rückwärtsfahren nach hinten; Roll-Formel
  Strecke / WheelRadius * RollFactor; Antenne: unteres Segment dreht zur Federspitze (statt Spline).
- Bot-Wege (Bot-Log des Users: 132 "stuck ... attack", Hotspots, Bots 0,5 m vor dem Ziel an der Wand):
  Pfadglättung prüft jetzt Mitte + seitlich und die Bodenstufen (walkable_line), Knoten an Wänden
  kosten 0,4 m pro fehlendem Nachbarn, Steckenbleiben erhöht die Kosten dort für alle Bots (+15 m, max 300),
  letzter Wegpunkt gilt ab 0,8 m als erreicht (blockiert < 2 m davor = erreicht).
  Selbsttest HB_NAVTEST=300 (Crossroads, simulierter Bot-Walker): alt 225/300 angekommen,
  60 dauerhaft stecken, 189 Stuck-Ereignisse -> neu 271/300, 16, 48 (Seitenabstand 0,35 m: 0,25 -> 269, 0,45 -> 268).
- Ziel-Priorität: Bots mit capture/attack laufen beim Schießen weiter ihren Weg (kein Seitschritt),
  solange der Gegner > 8 m entfernt ist und sie > 8 m vom Ziel sind; Deckung unter Beschuss 35 % statt 65 %
  (alles NOT from data).
- Build-Warnungen bereinigt: f32-Literale in egui::Stroke::new (wird sonst künftig ein Fehler), unnötige mut, Dead-Code-Warnungen per #![allow(dead_code)] stumm (ungenutzte Datenfelder bleiben).

## Phase 6: Navmesh/Skill, Drohne bleibt + Leben, Drohnen-Feed, Wärmebild, Einstellungen (2026-10-19)
- Bots: Steigungsprüfung in walkable_line (Normale < WALKABLE 0,7 und Stufe > 5 cm = nicht begehbar),
  Selbsttest 277/300 angekommen. 3x stecken an derselben Stelle (1 m) = Bot wird zum günstigsten
  Knoten 2-8 m entfernt versetzt (Log "trapped at"), A*-Heuristik x1,3 (alles NOT from data).
- Bodendrohne: E verlässt sie, sie bleibt stehen (Parked, verbraucht fuel_idle), Slot-Taste steigt wieder ein.
  Leben 3x Spieler (Wunsch User; Daten hätten 730), KVA-Bots zielen auch auf sie, Trefferradius 35 uu,
  Zerstörung mit FX_C4_EXP (alles NOT from data außer HEALTH_MAX).
- Hellfire-HUD: VehiclePlacerPredator (PredatorDrone, AirstrikeCounter "ÜBRIGE SCHÜSSE", Batterie).
- Debug-Menü: alle Killstreaks gratis (Spieler-Tab).
- Drohnen-Feed (videofx.rs/.wgsl): MAT_RD.postFX.M_VideoFX Knoten für Knoten als Vollbild-Pass vor der
  Post-Kette. Bodendrohne M_VideoFX_INST (Desaturation 0.8). Hellfire: CFG_Airstrike_Hellfire
  bThermalVisionWhenZoomed = Wärmebild -> GCObjectPool_Content DefaultPPThermalMaterial MAT_RD.postFX.M_Thermal01
  (M_VideoFX, Desaturation 1) und CharacterThermalMaterial CH_3P.M_Character_Thermal_White
  (unbeleuchtet, Color 1,1,1) auf allen Soldaten. Fahrzeuge: kein Materials_Thermal gesetzt.
  NOT from data: Effekt als Vollbild statt auf dem Cockpit-Mesh.
- Einstellungen (settings.rs, options.rs, input.rs), gespeichert in homebots_settings.ini (Projektordner,
  löschen = Standard). Widgets über ihr Data-Store-Markup, Combo-Einträge aus den Loc-Dateien
  (gcuidataprovider_*), Standards aus GCProfileSettings DefaultSettings und GCEngine.ini des Users.
  Wirkt: Lautstärken (Master/Musik/Geräusch/Sprache/VOIP über die SoundGroup-Zweige User_Master/MusicUI/
  SFXUI/Voice/VoiceChat), Gamma, Bloom, VSync, Fenster/Vollbild/Rahmenlos + Auflösung (mit "Übernehmen"),
  Texturqualität (Größengrenze 256..4096, ab nächster Karte), Anisotropie, Botanikdetails
  (= FoliageDrawRadiusMultiplier, ab nächster Karte), HUD-Schalter, FPS-Anzeige, Untertitel, Sichtfeld,
  Schwierigkeit, Maus (Empfindlichkeit, Horiz./Vert., ADS, invertieren, Glättung, Beschleunigung, je Tab
  Infanterie/Land/Luft), alle Tastenbelegungen (DefaultInput_deu.ini BindingsFoot/PCGlobal/Land/Heli;
  Linksklick = neue Taste, Rechtsklick = löschen, Esc = abbrechen).
  Ohne Wirkung (HomeBots hat es nicht): Schatten-/Shader-Qualität, AA, Weltdetails, Zeichendistanz,
  Bewegungsunschärfe, Feldtiefe, Störung, AO, DX11, Namen/Symbole über Köpfen, Joystick, Push-to-Talk.
  NOT from data: Master 80 (Wunsch -20 %), Fenstermodus als Standard (ini hätte Fullscreen=2), Texturstufe
  Mittel = bisherige 1024, Texturgrößen je Stufe, Schatten/Weltdetail-Standard, Gesamtqualität setzt die vier
  Qualitätsfelder, Schwierigkeit -> KVA-Skill 1,0/1,25/1,5/1,75 (Normal = 1,25), Maus-Regler 0/50/100 auf
  MouseSensitivityScaleLow/Med/High 0,25/1/4 bzw. Faktor 0..2, ADS-Faktor 0,40 (m_fZoomLookScale) x Regler,
  Beschleunigung bis x2, Glättung = Mittel aus 2 Frames, Sichtfeld-Regler als horizontales FOV,
  Klappliste-Optik, Lage der FPS-Anzeige, Zuordnung HUD-Schalter -> Widgets.
  Geändert ggü. vorher: Mausrad hoch = nächste Waffe (ini), Ducken nur LStrg (C = Kamera laut ini),
  Glättung an (ini bEnableMouseSmoothing), beim Zielen langsamere Maus (m_bUseZoomLookScale).
- UI-Layout: Dock-Einträge mit falscher Ausrichtung (Oben an Rechts) werden ignoriert
  (U_HUDPrefs Untertitel-Häkchen war ein schwarzer Balken).
- Nachtrag: Lautstärken wirken jetzt sofort auch auf laufende Sounds (Menümusik, Loops): jede Stimme
  merkt sich ihre Datenlautstärke und Klasse, bei einer Änderung wird neu gesetzt.
  Einstellungen werden 1 s nach der letzten Änderung gespeichert (auch Tastenbelegung).
- Debug-Menü Killstreaks: Namen aus GCLoadoutItemInfos strItemDisplayName bzw.
  GCGameContent.<Fahrzeug>.VehicleNameString (+ USA/KVA), in HomeBots nicht umgesetzte grau.
  BMB_SensorFused hat keinen Namen in den Daten.
- Wärmebild-Goggles (CFG_EQP_Thermal): GCWeap_EQP_Thermal MaxThermalDuration 180 s, Ein/Aus mit der
  Slot-Taste, Sounds ThermalOnSC/ThermalOffSC (GCWeap_EQP_Thermal_C), Nachbild DefaultPPGogglesMaterial
  M_Thermal_Goggles (TintColor 0.75/1/0.75, Rahmen ThermalGoggles_corner01). NOT from data: einmal pro Leben
  kaufbar, Countdown läuft nur eingeschaltet, Soldaten weiß wie beim Hellfire (CharacterGogglesMaterial nicht
  einzeln nachgebaut).
- Wärmebild nur für Gegner (Wunsch User): eigene Teamkameraden bleiben normal (NOT from data).
- Nachschub (CFG_EQP_Resupply): füllt Primär, Sekundär, aktuelle Waffe, Splitter- und Taktik-Granaten
  (UI_Armory Resupply_Text; ResupplyAmmo ist nativ, nicht dekodiert -> Umfang NOT from data).
- Luftschläge Cluster / Weißer Phosphor / Sensorgezündet: CFG_BMB_* -> GCProj_Airstrike_* (BombCount,
  PayloadReleaseDistance, StrikeWidth/StrikeRadius, SubmunitionClass, AirburstHeight, StrikeDuration,
  ProjExplosionTemplate, BombletExplosionTemplate). NOT from data: Cluster-Bomblets mit 0..0,4 s
  Zufallsverzögerung, gleichmäßige Verteilung auf der Kreisfläche, Sensorgezündet zielt auf Gegner im
  Radius (sonst zufällig), Zielpunkt per Bodenstrahl.
- Panzerabwehrdrohne MQ60 Rhino (CFG_DRO_GAntiTank -> GCV_SH_GAntiTankWalker_Content, Werte über der
  Elternklasse GCV_SH_GAntiTank_Content): Mesh SKV_UCV_GAntiTank, gleiche Fahrsteuerung/Rig wie die
  Angriffsdrohne, HUD VehiclePlacerGroundAntiVehicle. Waffe MP.Vehicle.CFG_GAntiTank: Linksklick
  (EKBC_Fire) FireModes[0] GCProj_GAntiTankDumbfire_Seat0_Missile_C (Speed 3500, DamageRadius 200 aus
  GCProj_MissileFireAndForget_Content), 1 Schuss, 3 s Nachladen; Rechtsklick (EKBC_AltFire) FireModes[1]
  GCProj_GAntiTank_Missile_C, Salve 4 im Abstand 0,25 s, 3 s Nachladen. Abschuss an den Sockets
  Seat0FireMode<m>Socket<k>. Kamera am Socket DriverViewSocket (Turret_Pitch) + CameraOffset (-35,0,0).
  NOT from data: kein Lock-on (bUseLockOn von Modus 1), Raketen fliegen gerade, Flugkörper-Mesh = AT-Rakete,
  Kauf einer anderen Drohne gibt eine geparkte auf.
- Projektilwerte folgen jetzt der Klassenkette im Paket (fehlende Werte aus Elternklassen statt Standard).
- Angriffsdrohne: Kamera jetzt ebenfalls am DriverViewSocket + CameraOffset (0,0,50) = 17 uu höher als vorher.
- Drohnen allgemein aus den Daten: Fahrzeugklasse = GCWeap_DRO_<X>_C m_DroneVehClass, Werte über die
  Elternklassen, Waffe = Seats[0].GunClass -> VehicleWeaponInfo, Sound = VehicleAudio, Treibstoff =
  VehicleFuelInfo (Werte über Default__GCVehicleFuelInfo), HUD-Placer nach deren VehicleTypes (EVT_A<Klasse>).
- Luft-Angriffsdrohne MQ-50 Buzzard (CFG_DRO_AAssault -> GCV_SH_AAssault_Content): SKV_UCV_AirDrone,
  AirSpeed 425, Treibstoff AirAssaultFuel (10/s fliegen, 5/s schweben, 2 je Schuss), Waffe
  MP.Vehicle.CFG_SH_AAssault: GCProj_AAssault_Seat0_Missile_C, Magazin 2, Abstand 0,2 s, 2 s Nachladen,
  Abschuss an Seat0FireMode0Socket0/1. HUD VehiclePlacerAirAssault.
- Luft-Aufklärungsdrohne MQ-50 Parrot (CFG_DRO_ARecon -> GCV_US_ARecon_Content): SKV_SD_UCV_ParrotRecon,
  AirSpeed 390, Treibstoff ReconFuel (15/s, 10/s), keine Waffe (CFG_SH_ARecon EWFT_Custom). Gegner im
  Umkreis sqrt(m_nDetectionRadiusSq) = 1500 uu (GCVeh_RCDroneHeli; Buzzard setzt 0) erscheinen auf der
  Minimap, solange sie draußen ist. HUD VehiclePlacerAirRecon.
  NOT from data (beide Luftdrohnen): Steuerung W/S entlang der Blickrichtung inkl. Neigung, A/D seitlich
  (Land-Belegung wie die Bodendrohnen), Kollision per Strahl mit Abstand = Zylinderradius, schwebt beim
  Verlassen an Ort und Stelle, keine Rotor-Animation, Bedeutung der Detection-Reichweite als Minimap-Aufdeckung.
- Luftschlag-Zielmarkierung (Default__GCV_AirstrikeUAV_C StrikeMeshComponent MAT_RD.SpecialCase.UI.Targeter02,
  Ring + Kreuz): die Oberseiten des Meshes werden auf den Bildschirm projiziert und als HUD-Flächen gezeichnet.
  NOT from data: Darstellung als HUD statt als Mesh mit M_laserSphere01 (dieser Weg zeichnete hier nichts),
  Farbe Weiß mit Deckkraft 0,75, Größe = Schadensradius (Hellfire 600) bzw. Streufläche der Bomben,
  Ring auf den Boden gelegt.
- Abgleich mit dem Gameplay-Video des Users (Killstreaks):
  - Drohnen starten in der Verfolgerkamera (Video: Angriffsdrohne, Buzzard, Parrot von hinten): Kamera
    Seats[0].BehindCameraMaxOffset vom DriverViewSocket (Bodendrohne -200/0/0, Rhino -150/0/50, Buzzard
    -200/0/50, Parrot -150/0/20), bei Hindernis davor eingezogen. C (EKBC_BehindView) schaltet auf Ich-Sicht
    mit Cockpit-Mesh (CockpitMesh Interior_UCV_GAntiTank bzw. Interior_UCV_AAssault, Translation, im
    Vordergrund), Drohnenkörper dann ausgeblendet (bDisable3pMeshIn1p).
    NOT from data: Startansicht hinten (Video), Abstand beim Einziehen 10 cm.
  - Drohnenbild farbig wie im Video (kein grauer Video-Filter mehr; M_VideoFX_INST liegt auf dem
    Cockpit-Glas, das nicht gezeichnet wird). Grau bleibt beim Luftschlag.
  - Zielbox (GCUIObject_TargetingBox) aus LockOn.BigRectangle der Drohnenwaffe: Rhino 320x150,
    Parrot 240x120 (1280x720). Farbe HUD-Grün, Linienstärke 2 (NOT from data).
  - Parrot markiert: Gegner in der Zielbox, in Sichtlinie und innerhalb sqrt(DroneLockOnDistanceSQ) =
    3162 uu (GCLockOnTargeterRecon) bekommen die weißen Ecken (ReconDroneMarkers_UnMarked an den
    SmallRectangle-Ecken), Feuer halten (Hinweis ReconDroneMarkTarget "HALTEN, UM EIN ZIEL ZU MARKIEREN")
    markiert sie für DroneMarkDuration 10 s: rote Raute ReconDroneMarkers_Marked 10 uu über dem Kopf
    (ReconDroneMarker PositionAdjustment) in jeder Ansicht und auf der Minimap. Ersetzt die
    Umkreis-Aufdeckung von vorher. NOT from data: Rautengröße 45 (Video), kein Blinken.
  - Luftschlag-Drohne kreist laut GCV_AirstrikeUAV: Mittelpunkt 1500 uu vor dem Spieler, Radius 9000,
    Höhe 5000, FlySpeed 0,02 (als rad/s gedeutet, NOT from data), Blick dreht mit. WASD-Steuerung entfernt.
    Rechtsklick hält Zoom (bAllowZoom, Hinweis AirStrikeZoom "ZUM ZOOMEN"; halbes Sichtfeld NOT from
    data), Wärmebild nur gezoomt (bThermalVisionWhenZoomed).
  - Zielmarkierung: Hellfire so groß wie DamageRadiusFull 200, Bomben wie StrikeWidth 800 der Streubombe
    (Video, NOT from data für Phosphor/Sensor), von Gebäuden verdeckt wie im Video, Linienstärke aus der
    Projektion.
- Explosionen (C4, Raketenwerfer, Drohnen-Raketen, Killstreaks):
  - Ursache "nur Sound": der Oberflächen-Eintrag (PM_Dirt usw. für GCDmgType_Explosion) hat nur Sounds,
    er galt trotzdem als gespielt. Jetzt: hat der Oberflächen-Eintrag keine Partikel, kommen sie aus
    PM_Default_Exp (GCObjectPool_Content ProjExplosionDefaultPM), erster Eintrag entlang der
    Schadensklassen-Kette MIT Partikeln (leere Einträge wie GCDmgType_RocketLauncher werden übersprungen):
    ATRL/PROX/Sensor -> FX_Rockets.ATW_Air, C4 -> FX_C4.FX_C4_EXP, Hellfire -> Hellfire_EXP + Imp_*_TankShell,
    Phosphor -> FX_WPH.Impact_WPH_MP, Drohnen -> ATW_Air / Frag_Air. NOT from data: diese Rückfall-Regel
    (der native Explosionscode ist nicht dekodiert).
  - ProjExplosionTemplate der Projektile wird gelesen (PROX: KFX_VHC.FX_MD600.FX_MD600_InAirExplosion,
    Phosphor-Granate: Grenade_WPH_MP); KFX_VHC wird dafür geladen (damit erscheinen auch Thunderball-
    und Rauch-Emitter von ATW_Air / Hellfire_EXP, die vorher fehlten).
  - Bomblet-Effekte: Pool so groß wie BombCount (bis 24), vorher 4 (Phosphor: 23 Bomben).
  - Materialknoten SkyLightContribution (Homefront) gab bisher die Normale als Farbe durch -> regenbogen-
    farbener Rauch/Feuerbälle (M_ConvectionFluid: C4-Feuerbälle). Jetzt Himmelslicht-Halbkugel wie GameMat
    (Upper/LowerSkyColor, (0.5±0.5 N.z)^2). NOT from data: Formel des Knotens selbst nicht dekodiert.
  - NOT from data (auf Wunsch "beeindruckend"): Raketen/Flugkörper und C4 werfen zusätzlich die
    Oberflächen-Trümmer der Hellfire (Imp_<Oberfläche>_TankShell) auf.
- Dynamische Punktlichter (dynlights.rs): Explosionsblitze aus GCExplosionLight_* TimeShift (StartTime, Radius,
  Brightness, LightColor; linear interpoliert, nach dem letzten Schlüssel aus), Abfall
  pow(1-(d/R)^2, FalloffExponent 2) (Engine Default__PointLightComponent). Beleuchtet GameMat-Objekte,
  Terrain und Straßen (Game-Shader). NOT from data: welche Klasse welches Projektil bekommt (C4 -> C4,
  Hellfire -> TankDES, Raketen -> Rocket, Granate/Flashbang/Phosphor-Bomblet -> Grenade, Streubombe ->
  ClusterBomb, Bomben-Auslösung -> ClusterBomb; GCProj_Airstrike übergibt LightClass None), Straßen nehmen die
  Base-Pass-Farbe als Albedo, Terrain-Normale aus Ableitungen.
- C4-Blinklicht: rotes Punktlicht am Sprengsatz, Farbe aus Lights_C4 (ColorOverLife 25/1/1),
  MatControlledFlickerSpeed 2 -> 2 Hz. NOT from data: an 35 % der Periode, Radius 450 uu, Helligkeit 6
  (das Original-Material leuchtet nur 48 uu; auf Wunsch stärker).
- Test-Hooks: HB_TEST_BOOM=<cfg>[,<s>[,<m>]] (Explosion vor dem Spieler), HB_TEST_LIGHT=<hell>,<radius>,
  HB_FXONLY=<Emitter>, HB_LIGHTDBG.
- Killstreak-Rückmeldung des Users (Video 2026-10-19):
  - Splash-Schaden: die Explosion trifft Soldaten nach dem Abstand zu ihrem Kollisionszylinder
    (UE3 HurtRadius nimmt Akteure, deren Kollision den Radius berührt), nicht zur Mitte; bei
    DamageRadiusFull > DamageRadius (GCProj_AAssault_Missile_C 50/150, GCProj_PhosphorusBomblet_C
    300/500) begrenzt der größere Radius die Explosion (vorher traf sie fast niemanden). Gilt auch für C4.
  - Rauchspur der Raketen: GCParticleModuleTypeDataTrail (Homefront) wird gelesen (TrailMaterial,
    bRenderSprites, UTiling, Sheets, SpriteFrequency); die Trail-Emitter wurden bisher wie Mesh-Emitter
    übersprungen. Partikel entstehen zusätzlich zur Rate entlang des Flugwegs (NOT from data: alle 24 uu,
    der GC-Trailcode ist nativ), bis zum Einschlagpunkt. Gezeichnet als Sprites mit dem Emitter-Material
    (das Band mit TrailMaterial ist unfertig, HB_TRAIL_RIBBON=1).
  - Nicht gebackene Verteilungen (bCanBeBaked false, leere LookupTable): jetzt aus dem Distribution-Objekt
    (Constant / Uniform / ConstantCurve, InterpCurve nativ serialisiert: Punkte InVal, OutVal, Tangenten,
    InterpMode) – die Raketen-Rauchfarbe war dadurch schwarz (ST_ATW_Rocket ColorOverLife: orange
    (10,4,0.77) -> weiß bei 0,15 Lebenszeit). Kurven linear zwischen den Punkten (Tangenten NOT from data).
  - Luftdrohnen-Steuerung (Wunsch): W/S vor/zurück in Blickrichtung (waagerecht), A/D seitlich,
    Leertaste steigen, Strg sinken (Tasten aus BindingsHeli EKBC_Ascend/EKBC_Descend). NOT from data:
    weiches Beschleunigen/Abbremsen (2,5/s) und Neigen in Flugrichtung (bis 14°).
  - Rotoren: GCSkelControl_HeliRotorSwap (AT_UCV_AAssault / AT_UCV_ParrotRecon): drehender Zustand
    (höchste RotorRPS) mit BoneScale (0 blendet ROTOR_MAIN aus, ROTOR_MAIN_BLUR an bzw. Static_Rotors aus)
    und vMaxRotationsPerSecond (Buzzard 5/3, Parrot 8), bSpinWhenNotDriven. NOT from data: Vektor-
    komponenten als Gier/Nick/Roll-Drehrate gedeutet.
  - Parrot-Markieren: +20 KP je neu markiertem Gegner (GCBattlePoints Rewards PSR_PaintTarget 20,
    GCGAMEEVENT_DRONE_PAINT_TARGET) mit Banner.
  - Test-Hooks: HB_TEST_ROCKET=<cfg>[,<s>], HB_TRAILDBG.
- Killstreak-Rückmeldung 2 (Video 2026-10-19 15-22-07):
  - Sounds in den Karten: die A2_*-Audiopakete (A2_Droids_C/_S, A2_PreditorDrone_C/_S, A2_Airstrike_C,
    a2_ah64_c, A2_Destruction_C …) haben keine eigene Datei, sie sind in FL-<Karte>.map gekocht; die
    deutschen Sprachwellen (A2_Airstrike_S, A2_BC_S) in FL-<Karte>_LOC_deu.upk (Crossroads fehlt in der
    Installation, die Wellen liegen identisch in FL-Overpass_LOC_deu.upk). Neu: Suche in diesen Dateien
    (Pfad "Paket.Gruppe.Name"). Dadurch jetzt aus den Daten: Drohnen-Motor (Buzzard
    SC_WPR_DEP_RCDroneHeli_Idle, Parrot SC_ParrotDrone, Bodendrohnen SC_GroundDrone_Engine), Drohnen-Schuss
    (SC_HeliDrone_Fire, SC_GroundDrone_M249_Fire3P), Hellfire-Flug SC_HellFire_Fire_MP (AmbientSound).
  - Luftschlag-Ton: VehicleAudio der UAV-Klasse (GCV_AirstrikeUAV_C / _Cluster_C -> AudioSetup_PredatorDrone,
    _Phosphorus_C -> _Phosphorus, _SensorFused_C -> _Carpet): Motor SCEngineIdle SC_PreditorDrone_Idle
    (Schleife im Cockpit), Abschuss SCFireSound SC_AH64_Missile_Fire.
  - Funkerstimmen: KAOSAIChatter.MPChatterConfig GCUtt_MPAirstrike* (Line = USOPERATOR-GCSoundCueInstance):
    PredatorTransition beim Start, FirstHellfire / SecondHellfire, CarpetBombFired, WhitePhosphorousFired,
    AirstrikeKill / MultiKill / Miss, BingoFuel. GCSoundCueInstance: Master-Cue (SC_MasterBC_Random:
    SoundNodeConcatenator Funk-Klick -> Stimme -> Klick) mit den Wellen der Instanz in den
    SoundNodeWaveParam-Plätzen, VolumeMultiplier, SoundGroup. NOT from data: Streubombe nutzt die
    CarpetBomb-Zeile, BingoFuel bei 20 % Treibstoff, Treffer/Fehlschuss-Zeile 0,6 s nach dem letzten
    Einschlag (Kills des Spielers seit dem Abschuss).
  - SoundNodeDistanceCrossFade (13 Explosions-Cues, z. B. SC_Exp_HellFire: 4 Schichten nah / mittel /
    fern / Nachhall nach Hörerabstand, FadeIn/FadeOut min..max): wurde bisher wie ein unbekannter Knoten
    nur mit dem ersten Kind gespielt – der Hauptknall und die Fern-Schichten fehlten.
    SoundNodeConcatenator: Kinder nacheinander (Wellenlänge aus Duration).
  - Explosionsschaden: Sichtprüfung von der Explosion zu Mitte, Kopf (+30) und Knie (-28) des Zylinders,
    eine freie Linie genügt (NOT from data; UE3 prüft die Actor-Location). HB_DMGDBG loggt jede Explosion.
  - Rauchspuren / EMP- und Blendgranate: Trail-Emitter werden als zusammenhängendes Band gezeichnet
    (je Segment ein zur Kamera gedrehtes Viereck vom vorigen zum aktuellen Partikel, Breite =
    Partikelgröße, Emitter-Material), Sprites nur noch, wo bRenderSprites es verlangt (jeder
    SpriteFrequency-te). NOT from data (der GC-Trail-Renderer ist nativ). Vorher war jeder Partikel ein
    eigenes Sprite: dünne, lückige Spur; der EMP_Grenade_Trail (Farbe 15–30, nur Band) sah als Sprite-
    Haufen wie eine Detonation in der Hand aus.
  - Test-Hooks: HB_STRIKEDBG.
- Rückmeldung 2026-10-19 17:29 (Klicken, Bots erobern nicht, Profiler):
  - Klickgeräusch: (1) lewton gibt das letzte Ogg-Paket ganz aus, bis zu 1012 Samples mehr als die
    Granule-Position der letzten Ogg-Seite (= Duration × SampleRate der SoundNodeWave; 168 von 984
    Wellen in A2_Weapons_S / A2_M1114_S / A2_Droids_S / A2_CharacterMovement_S). In Schleifen brach das
    die Nahtstelle bei jedem Durchlauf (z. B. JeepIdle_mono, Humvee-Leerlauf: Klick alle 3,3 s),
    Einzeltöne endeten auf einem Wert ≠ 0. Jetzt auf die Granule-Position gekürzt (aus den Daten).
    (2) Gestoppte Stimmen (Kanallimit 64, MaxConcurrentPlayCount-Neustart, Feuerschleifen-Ende jedes
    Bots) wurden mitten in der Welle abgeschnitten; jetzt 3 Frames Ausblenden (×0,35 je Frame) vor dem
    Stopp. NOT from data (die Stopp-Rampe von XAudio2 ist aus der EXE nicht bekannt).
  - Ogg → PCM für Einzeltöne in einem Hintergrund-Thread (bis dahin spielt die Ogg-Fassung); Schleifen
    weiter sofort dekodiert. Vorher dekodierte der erste Abspielvorgang einer langen Welle im Frame
    (sfx bis 44,8 ms im Profil).
  - Bots / Eroberung: goal_for wählte das Ziel als `offen[slot % n]` aus einer nach der *aktuellen*
    Entfernung sortierten Liste; nach ein paar Metern war die Reihenfolge eine andere, die Angreifer
    wechselten bei jeder Neuplanung (4–6 s) zwischen A und B und kamen nie an (Bot-Log: KVA „goal
    attack A/B“ im Wechsel bei 17–36 m). Jetzt behält ein Bot sein Ziel, solange es offen ist; nach
    einem Hängenbleiben wählt er neu (NOT from data, das MP-Spiel hatte keine Bots).
  - Profiler: (1) die Emitter-Meshes der Partikel-Pools (jeder Effekt × Poolgröße) wurden sichtbar
    erzeugt und ruhende Pools nie versteckt: ~5000 leere Zeichenaufrufe im Transparent-Pass (7,5 ms).
    Jetzt versteckt bis zum ersten Partikel (Test: sichtbare Blend 3422 → 382, Add 1616 → 29).
    (2) Partikel-Emitter außerhalb des Sichtkegels (Kugel um die Partikel gegen 75° halben Winkel)
    werden versteckt und nicht neu geschrieben (125 Mesh-Uploads je Frame, allocate_and_free_meshes
    3,7 ms). (3) Bots außerhalb des Sichtkegels werden nicht neu gehäutet, das Mesh folgt nur per
    Transform. (4) Deckungssuche nur noch über die Gitterzellen im 12-m-Umkreis statt über alle Knoten;
    Wegplanung höchstens 2 ms je Frame. Alles NOT from data (Budgets).
  - Profiler-Zähler: transparent_items, transparent_pipelines; Scopes bot_plan, bot_cover;
    HB_PHASEDBG loggt sichtbare GameMat nach Blend-Modus.
- Lautsprechermast (PA-System, Wunsch des Users, NOT from data): auf Crossroads ein Mast neben der
  Überführung bei Ziel B (UE x 600, y 300, Boden per Strahl von z -400 nach unten, Gier 200°).
  Mast = STO_energy.highPower.Statics.telePole_Conduit (aus den Daten, mit Kollision); drei Trichter-
  Lautsprecher und Halter selbst gebaut (kein Megafon-Mesh in der Installation), Material
  M_signPostMetal_C. Ton: die erste .ogg in assets/pa (hanoi_hannah.ogg, aus der MP3 des Users) als
  Endlosschleife; Lautstärke 1 bis 5 m, linear auf 0 bei 55 m (UE3-Linearmodell) × Optionen-Lautstärke.
  Andere Datei: HB_PA_FILE=<pfad>, andere Orte: HB_PA_AT=x,y,von,gier[;...], Draufsicht HB_PA_TOPVIEW.
- Atombombe (Killstreak-Wunsch des Users; das Spiel hat keine): alles NOT from data außer wo „Daten“
  steht. Neu nach dem Referenzvideo des Users (Modern-Warfare-Atomschlag) statt der ersten, comichaften
  Fassung (prozedurale Kugel-Wolke, Tuschebilder, Speed-Lines, synthetische Sirene / Klänge entfernt):
  - Loadout ATOMSCHLAG (7. Eintrag im Einsatzmenü, Kopie von wMenuLabel5) = erstes Standard-Loadout mit C4
    (Daten), Taste 4 = CFG_Nuke; auch im F1-Debugmenü (Killstreaks).
  - Ablauf ab Tastendruck: Zünder (Daten: CFG_EXP_C4Detonator) wird gezogen und gedrückt; 14 s Countdown im
    Banner („TAKTISCHER ATOMSCHLAG IM ANFLUG“) mit dem Countdown-Piepen des Spiels (Daten: A2_UI_S
    CountdownBeep); 1,6 s: Fulton-Ballon steigt an seiner Leine auf 150 m, der Blick bleibt auf dem Ballon,
    ab 3,2 s auf dem anfliegenden Transportflugzeug (C-130-Form aus Grundkörpern, im Spiel gibt es kein
    solches Flugzeug) bis es bei 7,2 s die Leine greift (unter einem Dach steigt der Ballon durch die nächste freie Stelle im
    Umkreis von 40 m); Ruck (Daten: S_VehicleFX_Large_Impact1), der Spieler
    hängt unter / hinter dem steigenden Flugzeug (65 m/s, Leine schwingt nach hinten), Fahrtwind und Triebwerke
    (Daten: S_VHR_JET_fighter_LOD langsam als Luftrauschen – die Windwellen der Karten sind in der
    Installation leere 261-Byte-Hüllen –, S_VHW_JET_Bomber_LOD01_3p / _2_Idle03, tiefer abgespielt), der Blick dreht zum
    Zielpunkt; 12,6 s: Sprengkopf fällt mit ST_Hellfire-Rauchspur und RocketFlyby (Daten).
  - 14 s Detonation: Blitz, GCExplosionLight_Nuke (Daten: GCGame) + Feuerball-Licht, Spiel-Explosionen am
    Boden und Staubstöße entlang der Druckwelle (Daten-Vorlagen). Wolke volumetrisch (nukecloud.rs /
    nukecloud.wgsl): Raymarching durch eine Box um den Nullpunkt, an der Szenentiefe (Szenenkopie)
    abgeschnitten; 64³-Perlin-Worley-Rauschen; Feuerball wächst in den rollenden Pilzhut (Torus, Rauschen
    rollt um die Röhre), Kuppel, Stamm, Base Surge am Boden, Kondensationsschale (Wilson-Wolke); Licht:
    Sonne der Karte mit Selbstschatten, Himmelslicht, Glut innen; Höhe ~520 m, Druckwelle
    S = 60 + 330·t^0,8 m (tötet alle Bots darin). Knall mit Schallgeschwindigkeit (343 m/s), die Explosions-
    wellen des Spiels tief abgespielt (Daten: S_spp_Explosion_02, ExplosionDistorted_04, GasTankExplosion01,
    PhosphoursExp_Big, AirPressure, Shell, TinnitusPitch); Feuerball 1,4 s sichtbar, dann Weißblende
    (1,8 s rein, 2 s halten, 2,2 s raus), danach Blick aus ~950 m Entfernung auf die wachsende Wolke;
    7 s nach der Detonation ist das Match gewonnen (der Spieler wurde abgeholt und stirbt nicht).
  - Test-Hooks: HB_NUKETEST=<s>[,<t0>], HB_NUKE_AT=x,y,z, HB_NUKE_HOLD=<s>, HB_NUKE_NOWIN, HB_CLOUDDBG,
    HB_NUKEDBG.
- Rückmeldung 2026-10-19 19:16:
  - Lautsprechermast: war unhörbar, weil er am Geräusche-Regler hing (beim User auf 0). Jetzt am Musik-
    Regler (Wunsch). Die Lautstärke-Regler selbst folgen den Daten (DefaultEngine.ini SoundGroups:
    SFXUI = SFX mit Waffen, Explosionen, Fahrzeugen, Ambiente, Schritten, Spiel-UI; MusicUI = Musik; Voice
    = Funk/Kommandeur); HB_SFXDBG loggt je Cue Gruppe und Regler.
  - Übernehmen-Knopf: Vollbild mit VideoModeSelection::Current wurde von winit abgelehnt („Could not find
    valid fullscreen video mode“). Exklusives Vollbild mit dem Monitor-Modus verlor danach auf der AMD-iGPU
    des Users gleich beim Start das Grafikgerät („Parent device is lost“) – „Vollbild“ ist jetzt randloses
    Vollbild über den ganzen Monitor (NOT from data: das Spiel schaltet den Anzeigemodus um); zurück in den
    Fenstermodus ohne gewählte Größe: Startgröße.
  - Brücke: die zerbrochenen Fahrbahnstücke der Überführung (HW_2Lane_2way_Traffic_brokenSeg2..4) blockieren
    nur Schüsse (BlockZeroExtent), begehbar machen sie BlockingVolumes (FL-Crossroads BlockingVolume_10/12/
    13/14 u. a.). BlockingVolumes werden jetzt als Spieler-Kollision geladen (Daten: Default__BlockingVolume.
    BrushComponent0 BlockActors, kein BlockZeroExtent -> Schüsse gehen durch); GCVehicleBlockingVolume
    (nur Fahrzeuge) nicht.
  - Raketen leuchten im Flug (Werfer, Drohnen, Hellfire / Luftschlag): flackerndes orangefarbenes Licht
    hinter dem Geschoss, Radius 7 m. NOT from data (die GCProj-Klassen haben keine Lichtklasse).
- Rückmeldung 2026-10-19 22:54:
  - Zweites Match nach dem Atomschlag: riesiger Humvee ohne Kollision und leere Waffen. Ursache: Zustände
    aus dem alten Match (Spawn-Zähler in combat.rs / vehicles.rs, Sichtbarkeits-Listen in render_vehicles /
    render_bots), die beim Neuladen der Karte nicht zurückgesetzt wurden. Jetzt beim Matchstart (Phase
    Front) zurückgesetzt und wieder aufgebaut, sobald die Karte neu geladen ist.
  - Lautsprechermasten: drei Stück, je einer an jedem Ziel der drei Phasen (an der Brücke, in der Mitte
    und am letzten Ziel). Gesucht wird eine freie, ebene Stelle 6–16 m vom Ziel (Boden fast eben, nichts
    darüber innerhalb 9 m). Der an der Brücke steht jetzt neben der Straße Richtung KVA-Spawn statt in der
    Brücke. Orte NOT from data (das Spiel hat dort keine Masten).
  - Leistung (Profiler-Log 22:44): Partikel weiter weg werden seltener neu gebaut (ab 40 m jedes 2.,
    ab 80 m jedes 3. Bild) und außerhalb des Blickfelds ausgeblendet. NOT from data.
  - Atomschlag, Runde 3 (Wunsch: „Flugzeug sieht schlecht aus, Sounds nicht gruselig, Detonation zu schwach,
    Impact-Frame fehlt“). Alles hier NOT from data (das Spiel hat keinen Atomschlag); externe Inhalte vom
    User am 2026-10-19 freigegeben, Quellen und Lizenzen in assets/nuke/CREDITS.txt (keine Spieldateien):
    - Flugzeug: heruntergeladenes C-130-Modell (Sketchfab, manilov.ap, CC BY 4.0) statt der Grundformen.
      Umgewandelt in ein eigenes kleines Format (assets/nuke/c130.hbm, extmodel.rs). Die Lackierung (Blue
      Angels / Marines, Abzeichen) ist entfernt, fast schwarz neu eingefärbt mit dezenten Blechfugen
      (Wunsch: „militärisch, schwarz“). Die Propeller drehen sich (je Triebwerk eine Nabe, sichtbar
      langsam + Unschärfe-Scheibe), der Fulton-Bügel an der Nase ist ergänzt. Ohne die Datei wird die alte
      Grundform-Maschine benutzt.
    - Sounds: heruntergeladen (Pixabay-Lizenz), in Ogg umgewandelt, gekürzt und angeglichen
      (sfx.rs: „ext:<datei>“ lädt externe Ogg-Dateien; preload beim Matchstart, damit lange Wellen nicht
      ruckeln). Zivilschutz-Sirene („Civil Defense Siren“, Wunsch des Users) ab dem Anruf (leiser, je weiter man weggezogen wird, verstummt nach
      dem Knall); Propellerflugzeug-Überflug, dessen lautester Punkt auf das Einhaken fällt, danach ein
      Motor-Brummen in Schleife; Detonation: „Nuclear Explosion“ + „Huge Explosion“ mit Schall-
      geschwindigkeit, die Druckwelle beim Spieler, das Grollen der Wolke aus der Ferne. Die Spiel-Wellen
      für Flugzeug, Knall, Druckwelle und Tinnitus sind raus.
    - Impact-Frame zurück, realistisch statt Comic: 0,05 s weiß, dann 0,28 s „überlasteter Kamera-
      sensor“ – hartes Schwarz-Weiß (Feuerball und Lichtstrahlen weiß, Rest schwarz), Negativ/Positiv/
      Negativ, zerrissene Bildzeilen, Rauschen. Dazu ein Glühen des Feuerballs auf der Linse (Kern,
      Strahlen, waagrechter Streifen), Feuerball-Licht stärker.
    - Detonation größer: Feuerball bis ~180 m Radius, Pilz steigt auf ~600 m+ (vorher 135 m / 440 m),
      Bodenwelle (Base Surge) und Kondensationsschale deutlich dichter. Die Spiel-Explosionen am Feuerball
      und die Staubstöße an der Druckwelle sind raus (wirkten neben dem großen Feuerball wie Feuerwerk).
  - Testaufnahme (record.rs): HB_RECORD=<ordner>[,<fps>[,<von>[,<bis>]]] – die Spielzeit läuft in festen
    Schritten von 1/fps pro Bild (auch ein langsamer Rechner ergibt ein Video in echter Geschwindigkeit),
    jedes Bild der Atomschlag-Sequenz zwischen <von> und <bis> Sekunden wird als PNG gespeichert, danach
    beendet sich das Spiel. Daraus werden Video und GIF (ffmpeg). HB_NUKE_HOLD zählt jetzt Echtzeit.
    Das Linsenglühen sitzt jetzt auf dem Feuerball (stieg vorher nicht mit). NOT from data.
  - Krater (crater.rs, Wunsch 2026-10-20 00:40, NOT from data): wird getauscht, während das Bild nach der
    Detonation ganz weiß ist; danach sieht man die Zone zerstört. Innerhalb 380 m vom Nullpunkt ist alles
    Platzierte weg (Static Meshes, Wracks, Straßen, SpeedTrees, platzierte Partikel-Emitter), Gras bis 600 m.
    Das Gelände bekommt eine Schüssel (70 m Radius, 16 m tief, 4 m Rand) in den Render-Meshes (Höhen in
    ATTR_TERRAIN_VERT) und in der Kollision; Boden und stehende Gebäude sind bis 720 m verkohlt (schwarz in
    der Mitte, Ruß, verbranntes Braun am Rand, ausgefranster Rand mit Streifen weg vom Zentrum; über
    Globals misc.yzw im Gelände- und Static-Mesh-Shader). Ein neues Match lädt die Karte neu.
    Fix: das Flugzeug-Überflug-Geräusch nutzte dasselbe once()-Bit wie die Ballon-Suche und lief nie.
  - Neues Match nach Drohnen-Einsatz: riesige Drohnen (Geschütz-Drohne, Hubschrauber-Drohne) mitten in der
    Karte. Ursache wie beim Humvee: die Karte lädt neu, die Drohnen-Meshes werden neu (sichtbar, am Ursprung,
    in UE-Einheiten) gespawnt, der gespeicherte Sichtbarkeits-Zustand in drone.rs sagte noch „versteckt“, also
    wurden sie nie versteckt. Jetzt wird der Zustand beim Neuladen der Drohnen-Daten verworfen (gilt für alle
    vier Drohnen). Geprüft: Waffen/Arme (ViewModel wird pro Karte neu gebaut), Bots und eigener Körper
    (Fix 2026-10-19), Humvee (Fix 2026-10-19), Luftschlag-Marker (versteckt gespawnt), Atomschlag-Flugzeug
    (einmal beim Start, kein Karten-Objekt).
  - Absturz beim Start (User 2026-10-20 01:35, „Usage flags STORAGE … do not contain COPY_DST“): crater.rs
    schreibt jedes Bild in die Globals-Puffer, die ohne COPY_DST angelegt waren. Beide Globals-Puffer
    (Gelände, Game-Shader) haben jetzt COPY_DST, und crater.rs schreibt nur noch in Puffer, die das erlauben.
  - Eigene Arm-Animationen (vmanim.rs, NOT from data): assets/nuke/*.hba aus Blender (Rig der Spiel-Arme
    SKC_MP_US_1P), ersetzen während sie laufen die Arm-Knochen, die Waffe wird dann nicht gezeichnet.
    HB_VMDUMP=<datei> schreibt die aktuelle Arm-Pose als Referenz. Noch keine Animation eingebaut.
  - Atomschlag, eigene Animationen und Modelle aus Blender (Wunsch 2026-10-20 01:19 / 01:22 / 02:08; NOT from data,
    in Blender über die Blender-Verbindung gebaut, Arbeitsdatei lokal: Documents\HomeBots_local\nuke_anims.blend –
    enthält das Spiel-Rig, kommt nie ins Repo):
    - Fulton-Ballon neu: länglicher Ballon (7,2 m, oliv-schwarz) mit Kreuz-Leitwerk und Seilaufhängung
      (assets/nuke/balloon.hbm, Ursprung am Aufhängering), Nase in den Wind, bläst sich beim Steigen auf.
    - Einhaken (assets/nuke/hook.hba + hook_props.hbm), auf dem Arm-Rig des Spiels (SKC_MP_US_1P) animiert:
      die linke Hand hebt den Gurtring vor das Gesicht, die rechte hakt den Karabiner am Seil ein (Schnapper
      öffnet, klickt zu: A2_UI_S.Click, A2_Weapons_S.M4.M4WClip3_mono), kurzer Zug am Seil, dann zieht das Seil
      alles nach oben weg und der Ballon steigt. Ablauf neu: Zünder 0,3–1,1 s, Einhaken ab 1,2 s (1,67 s),
      Ballon ab 2,65 s, Flugzeug sichtbar ab 4,2 s, Einhaken in den Haken des Flugzeugs weiter bei 7,2 s.
    - Mittelfinger (assets/nuke/finger.hba, 3 s) ab 1,8 s nach dem Einhaken durchs Flugzeug: rechte Hand hoch,
      kurzer Stoß, halten mit leichtem Wackeln; Handfläche zum Spieler, Handrücken und Finger Richtung Bombe.
    - Während die Animationen laufen, folgt die Spieler-Blickrichtung der Atomschlag-Kamera (die Arme werden
      von ihr aus gezeichnet).
    - Einhaken neu (Wunsch 2026-10-20 02:5x / 03:01, NOT from data): der Karabiner hängt erst am Seil,
      die offene rechte Hand greift von rechts zu und schließt sich zur Faust ums Seil über dem Karabiner
      (Hand nicht mehr im Karabiner), der Karabiner steht schräg (Schnapper zum Ring), der Ring drückt den
      Schnapper auf und liegt danach im Korb des Karabiners; kurzer Zug, loslassen, das Seil zieht ihn
      senkrecht nach oben weg. Blick senkt sich beim Einhaken leicht und geht am Ende hoch zum Ballon.
      Schnapper-Klick bei 0,70 s, Zuschnappen bei 0,90 s. Die Props hängen im Spiel jetzt an einem
      Eltern-Objekt mit der Seitenskalierung der Arme (HookSpace, K wie viewmodel.rs), damit sie genau
      an den Händen sitzen wie in Blender. hook_props.hbm: 9 Teile (Karabiner, Schnapper, Hülse, Seil,
      Seil-Auge, Ring, Gurt, Naht, langer Gurt).
    - Mittelfinger: Arm weiter gestreckt (Schulter vor, Hand weiter weg), Mittelfinger gerade; läuft jetzt
      ab 2 s vor der Detonation bis der Bildschirm ganz weiß ist (Wunsch 2026-10-20 03:12), die Hand
      bleibt oben und wackelt im Haltebereich des Clips (vor/zurück). Das Seil weicht der erhobenen Hand
      aus (Gurt-Ende wandert nach links, bis das Seil 25 cm an der Hand vorbeigeht).
    - Kamera-Wackeln nach der Detonation steckt jetzt in der Atomschlag-Kamera selbst (auch Spieler-Blick), so
      wackeln Arme und Mittelfinger mit, statt im Bild hin und her zu springen (Test 2026-10-20 03:25: Hand
      bis zu 0,4 m versetzt bzw. aus dem Bild).

## Fahrzeuge: LAV, Panzer, Hubschrauber (2026-10-20, Wunsch 12:58)
- vehicles.rs neu für mehrere Fahrzeugklassen, vehload.rs liest sie aus den Daten: die kaufbaren
  USA-Fahrzeuge aus GCBattlePoints VehicleCosts (GCV_US_*): M1151 HUMVEE (GCVCar), LAV PIRANHA
  (GCV_US_LAV_Content -> GCVAmphibious), M1A3 ABRAMS (GCVTank), AH-700 SCOUT (GCV_US_MD600_Content ->
  GCV_SD_MD600_Content -> GCVAttackHeli), AH-64 APACHE (GCVAttackHeli). Werte über die ganze Klassenkette
  (GCGameContent, dann die Skriptklassen in GCGame: MaxSpeed 550 / AirSpeed 800 / 900, MaxSprintSpeed,
  SprintTimeMax, SprintRechargeRate, Health 1000 / 1200, m_DefaultPhysicalMaterial PMA_Heavy / Medium / Light).
  - Modell: SVehicleMesh SkeletalMesh (US_M1A3, SD_LAV25, SD_MD600_v2, US_AH64; die Hubschrauber liegen nur in
    <Karte>_FrontlinesData: Suche über den vollen Exportpfad), Sockets, AnimTreeTemplate.
  - Ausgeblendet: Teile der anderen Seite (CH_* / OA_CH_*) und zerstörte Teile (*_DEST außer *_PRE_DEST, DEST_*),
    jeweils mit Unterbaum. NOT from data: die Regel nach Knochennamen.
  - Räder: Wheels[] (GCVWheel_*: BoneName, SteerFactor, WheelRadius); Panzer/LAV drehen alle WHL_*-Knochen.
    Bodenhöhe = Radmitte + WheelRadius (HUMVEE 55,8 wie vorher), Radstand / Spur aus den Radknochen.
  - Sitze (Seats): CameraTag + CameraOffset, BehindCameraMax/MinOffset, ViewMin/MaxLimit, TurretControls ->
    SkelControlLists des AnimTree (ControlName -> Knochen: Turret_Rotate / Turret_Pitch bzw. Gun_Pitch,
    MG_ROTATE / MG_PITCH), TurretControllerMin/MaxLimit (Höhenrichtgrenzen je Turm-Drehwinkel),
    fTurret<Yaw|Pitch>MaxSpeed x SpeedModifier (als Grad/s, NOT from data: die Einheit).
  - Waffen: GunClass -> VehicleWeaponInfo (MP.Vehicle.CFG_*): EWFT_InstantHit (Feuerrate, Überhitzen, Streuung
    SpreadMin bzw. FireSpread, Tracer) und EWFT_Projectile (Panzergranate GCProj_M1A3_Seat0_Shell_C Speed
    90000 Radius 350, LAV-Rakete / Flugabwehr-Rakete, Apache-Raketen 8er-Salve und Luft-Luft-Raketen),
    AmmoClip, FireBurstCount, FireInterval, ReloadTime; Abschuss an SeatFireModes Sockets (abwechselnd).
    Feuermodus 0 = Linksklick (EKBC_Fire), 1 = Rechtsklick (EKBC_AltFire). Lock-on nicht umgesetzt (fliegen
    gerade). Geschosse über CombatData.vehicle_rockets als Waffeneinträge (wie die Drohnen).
  - Ton: VehicleAudio (GCVehicleAudio, auch aus allen Kartendateien, a2_ah64_c liegt in FrontlinesData):
    SCEngineIdle / Shutdown / Sprint, EMP, SCFireSound je Sitz und Feuermodus.
  - Rotoren: GCSkelControl_HeliRotorSwap (Knochen, BoneScale, vMaxRotationsPerSecond) wie bei den Drohnen.
- Fahren (NOT from data, PhysX nicht nachgebaut): Panzer/LAV drehen auf der Stelle mit
  m_fTreadMaxSpeedSteering / Spurbreite, Beschleunigung 260 uu/s²; Hubschrauber: W/Leertaste steigen,
  S/Strg sinken (BindingsHeli EKBC_Ascend/Descend), A/D gieren (EKBC_YawLeft/Right), Maus seitlich gieren
  (höchstens MaxYawRate), Maus hoch/runter neigt die Nase (höchstens fAHMaxPitch 45°), Nase runter = vorwärts
  (Beschleunigung 900 uu/s², Luftwiderstand 0,6/s, Steigrate 450 uu/s), bis AirSpeed; Rumpf als drei Kugeln.
- Kamera: Fahrer/Pilot C wechselt Ich-/Verfolgersicht (BehindCameraMaxOffset vom Drehpunkt: HUMVEE und
  Hubschrauber über dem Ursprung, Panzer/LAV über dem Kamera-Socket auf dem Turm); Schützensitze schauen
  durch ihren CameraTag-Socket + BehindCameraMinOffset (wie der HUMVEE-Schütze bisher).
- Schaden: Treffer auf den Spieler im Fahrzeug gehen ans Fahrzeug (Hit hat jetzt den Schadenstyp): Wert der
  Damage-Grid-Spalte der Panzerung (m_DefaultPhysicalMaterial), skaliert wie gegen den Soldaten (Entfernung /
  Explosionsabfall). Gewehrkugeln gegen PMA_Heavy = 0. Bei 0 Leben: Explosion (GCDmgType_VehicleExplosion-
  Effekt der Oberfläche), der Spieler stirbt, das Fahrzeug verschwindet (NOT from data: kein Wrack).
- HUD: die Placer, deren VehicleTypes die Fahrzeugklasse oder eine Elternklasse nennen (VehiclePlacerTank,
  ...LAV, ...LittleBird, ...AttackHeli) + deren Seat<n>Placer; Nachladebalken bei Geschützen statt Hitze.
- Spawn: SpawnPoints-Typ nach Fahrzeugart (GMST_TreadedVehicle / AmphibiousVehicle / LightHelicopter /
  AttackHelicopter, NOT from data: die Zuordnung), sonst GMST_Jeep; Hubschrauber ohne Frontlinie an den
  AirVehicle-Starts.
- Tests: HOMEBOTS_TEST_VEHICLE=<Klasse>, HB_VEHTEST=<s> (Skript: fahren / steigen, drehen, Turm, feuern;
  Start nach <s> echten Sekunden), Aufnahme mit dessen Uhr (record.rs), HB_VEHONLY=<Klasse>.

## Ketten-Animation Panzer + Bodendrohnen (2026-10-20, Wunsch "chains animated")
- EXE: Name 'TreadLocation' = FName 0xbce (Name-Tabelle FUN_00ddf640). Benutzt von
  FUN_0087b310 (GCTreadedVehicle) und FUN_00bb3d20 (GCVeh_RCDroneTank): jeden Frame
  TreadMIC.SetVectorParameterValue('TreadLocation', (0, G, 0, 0)) mit
  G = (1.0 - PoweredWheel[+0x128] * 0.0027777778) * TexPan2UnrealScale
  (Konstanten 1.0 @0x0137fd30, 1/360 @0x013beca0 aus der EXE gelesen).
- Daten: Default__GCTreadedVehicle TexPan2UnrealScale 2.0; M1A3 Left/RightTreadMaterialIndex 2/3,
  Left/RightPoweredWheel LMWheel/RMWheel (WheelRadius 25). Default__GCVeh_RCDroneTank
  TexPan2UnrealScale 5.0, Left/Right 2/1; GAssaultWalker Left/RightPoweredWheel NewWHL_L0/R0.
- Materialgraph M_US_M1A3_treads und UCV_GroundDrones M_treads: alle Texturen bei
  TexCoord0 + (0, TreadLocation.G * -1). Umsetzung: UV0 der Ketten-Sections wird pro Frame
  um -G verschoben (gleiches Ergebnis, Parameter im Material bleibt 0). Nur Slots deren
  Material wirklich einen VectorParameter TreadLocation hat (LAV ist auch GCTreadedVehicle,
  dort Slot 0 = Wanne -> nichts).
- NICHT aus Daten: +0x128 als CurrentRotation in Grad (Name nicht belegt; /360 im Code spricht
  für Grad). Vorzeichen der Drehung pro Mesh so gewählt, dass das untere Kettentrum beim Fahren
  nicht über den Boden rutscht (M1A3: V wächst unten nach vorne, Drohne: nach hinten;
  examples/treaddir). Panzer: Rad-Rotation = Kettenweg / WheelRadius des Antriebsrads. Drohne: ohne
  RollFactor (die gezeichnete Radrolle enthält RollFactor 1.5).
- Bugfix Material-Zuordnung SkeletalMesh: LODInfo[0].LODMaterialMap wird jetzt auch für LOD 0
  angewendet. Belege: SKV_US_M1A3 [0,5,2,1,2,3,4,6] -> die Ketten (Sections 4/5, V-gekachelt)
  bekommen die Klassen-Slots 2/3, vorher lag Anbau-/Licht-Material auf den Ketten;
  SKW_ACH_Acog_base [1,0] -> Gehäuse-Material auf dem 528-Dreiecks-Körper. Betroffen sonst nur
  SKV_CH_M1114_1P, SKV_SD_LAV25, SKW_ACH_Acog_Var1, Interior_US_LAV25_GlassBreak (Scan aller upk).
- Bodenhöhe Kettenfahrzeuge: jetzt der tiefste Vertex der Ketten-Sections (M1A3 56,4 statt
  48,9: der Mittelwert aller WHL_*-Knochen enthielt Antriebs-/Leitrad und die Kette lag 7,5
  Einheiten im Boden).
- Test-Hooks: HB_VEHCAM=<grad>,<abstand>,<höhe> (Außenkamera am Fahrzeug),
  HB_DRONEDRIVE=<s> (Drohne fährt im Test 2 s vor / 2 s zurück).
- GCSkelControl_TankTread (Ketten-Knochen TREAD_* folgen der Federung) nicht umgesetzt: die
  Räder federn bei uns nicht.

## Fahrzeuge an Steigungen (2026-10-20, Wunsch 18:47 "Humvee kommt keine Steigungen hoch")
- Testumgebung HB_SLOPETEST=1 (slopetest.rs, ohne Fenster, ~10 s): lädt die Bodenfahrzeuge
  aus den Spieldaten wie im Match und fährt sie mit Vollgas über künstliche Bahnen (Rampen
  5..45 Grad, 20 m lang; Stufen 5..45 Einheiten). Dieselbe Bodenlogik wie im Spiel
  (vehicles::ground_step, aus drive herausgezogen). HB_SLOPETEST_V=<Klasse>,
  HB_SLOPETEST_LOG=<Bahn>, HB_SLOPEDBG=1 (Kontakte der Wandkugeln).
- Vorher: HUMVEE blieb ab 25 Grad hängen, LAV ab 20, M1A3 ab 15 (bei steileren Rampen fielen
  LAV/M1A3 sogar durch den Boden). Ursache: die Eckkugeln der Wandprüfung tauchen am Hang in
  den Boden und berührten die inneren Kanten benachbarter Bodendreiecke; deren
  Nächster-Punkt-Normalen kippen seitlich (n.y 0,42) und galten als Wand.
- Jetzt: Wände sind nur Dreiecke, deren Flächennormale steiler als drivable ist
  (|n.y| < 0,6, wie die alte Schwelle; CollisionWorld::push_sphere_steep). Danach schaffen alle
  drei alle Rampen bis 45 Grad. Stufen bis 30 Einheiten für alle, 45 Einheiten nur der HUMVEE
  (LAV/M1A3: die Wandkugel beginnt ~34 Einheiten über dem Boden). NICHT aus Daten: keine
  Verlangsamung am Hang (PhysX-Kräfte nicht nachgebaut).
- Test im Spiel: HB_VEHSLOPE=<grad> sucht im Umkreis von 150 m um den Spawn eine echte
  20-m-Steigung mit diesem Winkel, stellt das Fahrzeug bergauf hin und loggt alle 0,5 s.

## Klasse wechseln, Fahrzeuge kaufen, Debug-Fahrzeuge, Drohnenhöhe (2026-10-20, Wunsch 20:12)
- Pausemenü "KLASSE WECHSELN": der Knopf ist in den Daten ein GCCommand_UIMenuConfirmButton mit
  m_ActionCommand = PC_U_SpawnSelectionMenu (PC_UI_InGameMenus ChangeLoadout). Vorher wurde die
  Szene als Tod-Variante geöffnet ("Entstehen", kein Eingabe-Takt im Spiel -> Klicks wurden nie
  angenommen). Jetzt die Lebend-Variante: SpawnMebutton aus, QuickSpawnButtonPanel mit
  "Done" (Wahl gilt für den nächsten Spawn) und "Suicide" (Wahl übernehmen, sofort sterben ->
  Spawnauswahl). Countdown ausgeblendet. NICHT aus Daten: was Done/Suicide im Original genau
  auslösen (native GCUIScene_SpawnSelection).
- Spawnauswahl merkt sich den gewählten Fahrzeugeintrag (MatchFlow.vehicle_sel), solange er
  bezahlbar ist; Kauf wird geloggt ("vehicle: bought"), zu wenig KP auch.
- Debug-Menü: Fahrzeuge (GCBattlePoints VehicleCosts) sind nicht mehr grau; Klick stellt das
  Fahrzeug nach dem Schließen gratis an die Position des Spielers (ein gefahrenes Fahrzeug wird
  ersetzt).
- Bodendrohnen stehen jetzt auf dem tiefsten Vertex ihrer Ketten-Sections (vorher Zylindermitte
  + Translation z -15: Ketten 5,9 Einheiten im Boden); Kamera-Sockel um denselben Betrag höher.
- Test-Hooks: HB_SPAWNVEH=<eintrag> (Spawnauswahl nach dem Tod), HB_TEST_CHANGECLASS=<eintrag>
  (8 s nach dem Spawn Klasse wechseln, Eintrag wählen, Selbstmord).

## Pipeline-Vorwärmen in der Spawnauswahl (2026-10-20, Wunsch 21:05 "Waffen ploppen beim ersten Spawn rein")
- warmup.rs: Bevy baut die Render-Pipeline eines Meshes erst, wenn es zum ersten Mal gezeichnet
  wird, und kompiliert sie im Hintergrund; bis dahin fehlt das Mesh. Solange die Spawnauswahl
  nach dem Laden offen ist, bekommt jede noch nie gezeichnete Kombination aus Material und
  Vertex-Layout (GameMat, TerrainEmu, HfMat; Waffen, Arme, Fahrzeuge, Drohnen, Projektile,
  Effekte, Teile der Karte außer Sicht) einen Stellvertreter 50 km hinter der Kamera (wird
  geclippt, kein Pixel). Beim Spawn werden sie entfernt. HB_NOWARMUP=1 schaltet es ab.
  NICHT aus Daten (Renderer-Detail).

## Fahrzeuge: Schaden, Mündungsfeuer, Hubschrauber-Flugmodell (2026-10-20, Wunsch 22:10)
- Schadensraster: Schadenstypen ohne eigene Zeile nehmen die Zeile der nächsten Elternklasse
  (GCDmgType_LAV_Seat0_Bullet und GCDmgType_AH64D_Seat1_Bullet -> GCDmgType_Veh_30mm;
  Kette aus GCGame.u/GCGameContent.u). Vorher machten LAV-Kanone und Apache-Kanone 0 Schaden.
  NICHT aus der EXE belegt, dass das Spiel die Elternkette nimmt (sonst wären die Waffen nutzlos).
- Splash der Sofort-Treffer-Waffen: SplashDamageClass / SplashDamageRadius /
  SplashDamageRadiusFull aus den Klassen-Defaults (Veh_30mm -> Veh_30mmSplash, LAV 200/150,
  Veh_Bullet_Medium 150/80). NICHT aus Daten: linearer Abfall zwischen Full und Radius, der
  direkt getroffene Gegner bekommt keinen Splash. FullDamageRange der FireModes wird benutzt.
- Mündungsfeuer: die GC-Sockets haben ein References-Array (StartTag, Reference, ValidView);
  die SeatFireModes[].FlashTags (z. B. Seat0FireMode0Fire) starten die Partikel am Socket
  (LAV TurretFireSocket -> KFX_VHC.FX_LAV.FX_LAV_MF, M1A3 MF_Tank_3P, HUMVEE 50Cal_3P ...).
  VIEW_1P-Einträge und Lens-Flares ausgelassen. Einschlag-/Splash-Effekte der Fahrzeugwaffen
  kommen jetzt in die Effekt-Pools (vorher nur die HUMVEE-MG).
- Hubschrauber-Flugmodell aus der EXE (GCVehicleSimChopperBase::UpdateVehicle, Spieler-Zweig
  0x0071bf45..0x0071d47d): Kollektiv W/S entlang A = normalize(Z.x, Z.y, Z.z - 0.4), passiver
  Auftrieb PassiveLiftFactor*A (+2,25 %), Kollektiv-Kompensation, Luftwiderstand
  AirSpeed*AirResistanceMultiplier, A/D Gier-Moment TurnTorqueFactor, Maus x Roll-Moment
  RollTorqueStrafeFactor, Maus y Nick-Moment PitchTorqueFactor (pro Frame in Rotator-Einheiten,
  geklemmt), Dämpfungen, Auto-Nivellierung, StayUpright-Federn (Winkel/Steifigkeit aus den
  Sim-Defaults). Werte aus GCVSimChopper_AttackHeli / _MD600.
  NICHT aus Daten: Kraft-Skala a = F*g/(1,0225*PassiveLiftFactor) (Schweben), Drehmoment / 
  InertiaTensorOverride, g = 750, zusätzliche Dämpfung linear 0,35/s, Nick/Roll 2,5/s, Gier
  1,2/s (PhysX-Körperdämpfung nicht entschlüsselt), Euler-Integration der Lage.
- Prüfstand HB_SLOPETEST=heli: Start, Schweben, Nase runter, Gieren, Rollen, Sinken (AH-64, MD600).
- Kameras (Script GCVehicle VehicleCalcCameraThirdPerson / FirstPerson entschlüsselt):
  ESeatAllowedViews wählt die Ansicht (BOTH: C wechselt; Fahrer startet hinten, Schützen im
  Visier). Behind view: Drehung = Blickrichtung + BehindCameraRotation (Pitch); Drehpunkt =
  bCameraFixedToSocket3P ? Socket CameraTag3p/CameraTag + BehindCameraMinOffset (Fahrzeugraum)
  : Location + (0,0,50) + CameraOffset (Fahrzeugraum); Kamera = Drehpunkt +
  BehindCameraMaxOffset (bzw. MaxOffsetSprint beim Sprint) in der Kameradrehung; Blick entlang
  der Drehung. 1P: Socket CameraTag am Cockpit-Mesh (bUseCockpitMesh1p /
  bUseSecondaryCockpitMesh1p; m_CockpitMesh(Secondary) auf den Knochen des Außen-Mesh, mit
  Translation), sonst am Außen-Mesh; das Cockpit-Mesh wird in 1P gezeichnet, das Außen-Mesh mit
  bDisable3pMeshIn1p ausgeblendet. Rollen: 1P bei festen Sitzen/Hubschrauber, 3P nicht bei
  bCameraDoesNotRoll3P. NICHT umgesetzt: Kamera-Lag (CameraLag, MaxCameraRotLag), Orbit per
  Maus um den Drehpunkt bei festen Sitzen; Socket-Drehung angenähert durch Zielrichtung/Körper.
- HUD (PC_U_PlayerHudScreen): Sitz-Placer der anderen Sitze werden versteckt (SeatIndex);
  MilitaryHUD-Overlays nur in 1P; RangeToTarget zeigt die Entfernung statt "888.88 M";
  im Fahrzeug kein Infanterie-Fadenkreuz mehr (der Sitz-Placer bringt sein eigenes);
  VehiclePlacerMissileDefeat versteckt (Flares fehlen). Offen: Kompass, Fahrzeug-Icon,
  WeaponIcon aus UIIcon, leere Sitze "LEER".
- Test-Hook HB_VEHVIEWS=1 (mit HB_VEHTEST): 1P bei 3 s, Schützensitz bei 6 s, dessen Behind
  view bei 9 s.

## Bots erobern schlecht (USA), Bots frieren ein (2026-10-20, Bot-Logs 1792526583 / 1792532010)
- Deckung: der Weg zur Deckung endete auf einem zufälligen Knoten bis 4 m neben dem
  Deckungsknoten (plan_path), "in Deckung" galt erst unter 0,8 m -> der Bot stand dort bis zum
  Tod (Deckung gesetzt = kein Neuplanen, Stall-Wächter aus). Logs: 7 bzw. 11 Episoden mit
  60-131 s Stillstand (USA 607, KVA 724 Bot-s von ~1700/2000 im vorletzten Log; dort stand
  USA 113 s mit 3-6 von 7 Bots, Front 1 -> 0). Jetzt: Weg exakt auf den Knoten (plan_path_r
  Radius 0), "in Deckung" auch wenn der Weg zu Ende ist, nach COVER_TRAVEL_MAX 6 s Abbruch
  (NOT from data).
- Eroberungsziel: Ziel war Objective.Location + 1,5-6 m, dann ein Knoten bis 4 m daneben
  (bis 10 m vom Zentrum; die GameMode_FrontlineVolumes sind 7-15 m breit). Jetzt ein zufälliger
  Knoten der eigenen Komponente, auf dem ein stehender Bot im Volume liegt (gleiche Prüfung wie
  objectives.rs), Wand-/Kanten-Knoten (> ZONE_WALL_MAX 2 fehlende Nachbarn, NOT from data) nur
  wenn es keine anderen gibt. Weg exakt dorthin.
- Im Volume des eigenen Ziels (capture/attack/guard) = angekommen: kein Stall-/"no progress"-
  Reset (die Hälfte aller Resets passierte IM Ziel-Volume: USA 12/17 bzw. 14/20, KVA 13/26 bzw.
  13/27; der Reset erhöht bias -> Bot lief zum nächsten Punkt), Steckenbleiben dort = Ziel
  erreicht, Seitschritte im Gefecht bleiben im Volume.
- Aufteilung: goal_for bekam den globalen Slot (USA 0-6, KVA 7-14) -> "jeder dritte bewacht"
  ergab 3 von 8 KVA-Wächtern (8, 11, 14), aber 2 von 7 USA. Jetzt Index im Team (beide 2 Wächter).
- Bot-Log: Snapshot-Kopf mit Besitzer + Fortschritt je Punkt ("A:USA B:-<KVA 12/20> C:KVA"),
  pro Bot "zone X cover true/false".
- Offen/NICHT geändert: KVA-Skill (Schwierigkeit, settings.rs kva_skill 1,0-1,75) teilt
  Reaktionszeit und Streuung; im vorletzten Log starben USA-Bots 48x, KVA 27x.

## Fahrzeuge: Anbauteile, 3P-Zielen, Sitzwechsel, Apache-Cockpit (2026-10-21)
- Spiegel-Anbauteile: die StaticMeshComponents der Content-Klassen (AH64/MD600 wingleft /
  wingright: Stummelflügel, Raketenbehälter, Triebwerks-Seitenteile; LAV attachleft/right:
  Fahrwerksteile; M1A3 Mirror_Left/Right: Kettenschürzen) fehlten -> offene Schächte, darunter
  die Damage-Sektionen (MI_*_Damage, Glut) sichtbar ("wirkt zerstört"). Jetzt als starre Teile
  auf dem Bone des Attach-Sockets (Attach_Socket / AttachSocket; LAV/M1A3 Yaw 16384) mit
  Scale3D (-1 = gespiegelte Seite, Windung + Binormalen-Vorzeichen gedreht). Lage gegen die
  Rumpfgeometrie geprüft (examples/skobj + smverts, Python-Rendering). Nicht: *wreck
  (Zerstört-Zustand), HiddenGame (M1114 ragdollcollision). Drohnen-Raketenbehälter
  (GCV_SH_AAssault) noch offen (Socket unbekannt).
- 3P-Zielen: Schüsse fliegen jetzt von der Mündung zum Treffpunkt des Kamerastrahls (Kamera
  über/hinter der Waffe + BehindCameraRotation-Neigung -> Punkt traf nicht). Grenze 45° zur
  Waffenrichtung, Strahl bis 500 m (NOT from data).
- Sitzwechsel: F3..F8 (EKBC_Seat3..8, SwitchVehicleSeat 2..7) für Sitze mit Waffe oder
  Kamera-Socket; neuer Sitz übernimmt nicht mehr die Blickrichtung des alten (Turmsitz =
  Turmrichtung, sonst geradeaus).
- Apache-Cockpit schwarz: Interior_US_AH64_c ist in den Daten TC_Normalmap (LODGroup
  TEXTUREGROUP_Vehicle) -> matcompile entpackte es als Normal (x*2-1) -> schwarz. Der gekochte
  PS (RefShaderCache, examples/matps) entpackt nicht. Regel: TC_Normalmap mit Nicht-NormalMap-
  LODGroup und Name *_c = Farbtextur.

## Fahrzeug-Wracks, Schadensrichtung, Treffer-Sound (2026-10-21, Wunsch "Zerstörung / Schadensfeedback")
- destruction.rs (neu): ein zerstörtes Fahrzeug (vehicles.rs Health <= 0) wird ein Wrack statt zu
  verschwinden. Daten pro Klasse über die Default-Kette (Content + GCGame):
  - GCVehicle RagdollDefinitions (RagdollMesh, RagdollMaterials, RagdollAnimTree,
    VehicleEventsToFire), RagdollDefinitionListStationary / ...Moving. Script
    CalculateRagdollUseMovingList: VSizeSq(Velocity) > 50*50 -> Moving-Liste; Eintrag zufällig
    (CalculateRagdollInfoFromDefinitions, Rand). Wrack = Fahrzeug-Mesh mit den *_DEST-Materialien
    (M_Vehicle_DEST: Ruß + wandernde Glut) pro Material-Slot; AnimTree-Kontrollen
    SkelControlSingleBone BoneScale 0 (M1114 W4: Türen, Windschutzscheibe, Varianten-Kisten weg),
    an die Kinder vererbt. 2 Wrack-Instanzen pro Fahrzeugtyp, das älteste Wrack weicht.
  - Ereignisse: VehicleDestroyed (GCVehicle.Died) + VehicleEventsToFire der Definition (z. B.
    TurretLaunch / EngineFlip / TailBreak / OnGround): Socket-References des Meshes mit dem
    StartTag (Partikel, z. B. Vehicle_Civilian, Fire_WreckChassis; SoundCues, z. B.
    SC_FireLoop_Metal läuft bis zu seinem EndTag WreckExplosion), VehicleKismet
    GCSeqEvent_VehicleEvents -> SeqAct_PlaySound (SC_VehicleHusk) und
    GCSeqAct_FireVehicleEvent (TurretLaunch -> WheelPop -> Fire_WreckTire).
  - Zeitlauf aus dem Script (SetBurnOut, DyingVehicle.BeginState, WreckExplosionTimer):
    LifeSpan = RagdollLifespan (7 / LAV, M1A3, Helis eigene Werte), bei LifeSpan -
    m_fWreckExplosionStartTime VehicleEvent('WreckExplosion') (FX_VehicleRemoval_*) +
    GCVehicleAudio SCWreckExplosion, am Ende von LifeSpan weg. BurnOutTime / BurnTime-Parameter:
    M_Vehicle_DEST hat keinen solchen Parameter -> nichts zu animieren.
  - DyingVehicle.InitialVehicleExplosion: HurtRadius(ExplosionDamage 100, ExplosionRadius 200,
    GCDmgType_VehicleExplosion, ExplosionMomentum 60000, ExplosionRadiusFull 50) auf Soldaten /
    Spieler mit Sichtlinie; Schaden = Damage-Grid-Wert des Typs für PMA_Soldier (sonst
    ExplosionDamage), voll bis RadiusFull, linear bis Radius (wie die Granaten).
  - NICHT aus Daten: Wrack-Physik (PhysX-Körper der PA_*_wreck-Assets und AddImpulse
    TearOffMomentum/Mass nicht nachgebaut): Punktmasse, Abwurf 350 uu/s hoch + 150 uu/s weg vom
    Treffer + bis 0,6 rad/s Nick/Roll, Schwerkraft 750 (DefaultGravityZ), Bodenstopp auf
    Fahrhöhe (ride_height), Abprall 0,25, Reibung 3/s; Pool 2 Wracks pro Typ; Wrack in
    Referenzpose (GCSkelControlMovingAction-Verbiegungen der Räder nicht angewandt);
    Effekte am Socket mit der Wrack-Drehung (nicht der Knochen-Drehung); Rotor-Skalen wie beim
    fliegenden Heli.
  - Test: HB_TEST_VEHKILL=<s> (Fahrzeug des Spielers nach <s> s auf 0 Health), HB_WRECKDBG=1
    (Ereignisse loggen), HB_DMGDBG=1 (Explosionsschaden).
- dmgfeedback.rs (neu) + combat.rs apply_damage (DamageTaken) + hud.rs: Schadensrichtung wie
  PC_U_PlayerHudScreen HitDetector (GCUIObject_HitDetector: HitLocatorTexture Hit_Locator,
  HitDetectorSize 256x64, HitDetectorRadius 256, FadeDuration 4, MAX_HIT_LOCATORS 3,
  Ringpuffer, Richtung aus HitOrigin relativ zur Blickrichtung). NICHT aus Daten (natives
  Zeichnen nicht entschlüsselt): Mittelpunkt (640, 360), Bogenmitte auf dem Radius, gedreht,
  lineares Ausblenden.
- Treffer-Sound TakeHitSound SC_BulletImpact_Body_1P (Default__GCChar_MultiPlayer_Content), einmal
  pro Frame; m_HealthSoundCue SC_Health beim Unterschreiten von HealthDuckingStart 30.
  NICHT aus Daten: Auslöser von SC_Health (nativ).
- Nicht umgesetzt: Bildschirm-Effekt HealthPP_MP / ImpactPP_MP (GCBasePostProcess,
  M_TakeDamage_FahSho_INST FlashEffect / HealthEffect, HitEffectTime 0.5, MaxDamageEffect 80,
  GCPlayerInfo HealthEffectInfo; native Ansteuerung nicht entschlüsselt, 5 Texturen > 4 der
  UI-Materialbahn); Kamera-Wackeln bei Treffern.
- Autos der Karte (levelcars.rs, neu): FL-Crossroads hat 90 GCInteractiveSkelMeshActor
  (PrefabInstances VEH_Jiggle.cars.Prefabs.DSO_CivilianCar01_* / DSO_TruckCivilian03_* /
  DSO_UtilityVan_*) mit GCInteractiveObjectInfo (II_CarCivilian01 / II_TruckCivilian03 /
  II_UtilityVan: SkeletalMesh DEST_*, AnimSet AS_TruckCivilian03, OnShotAnim "shot" 0,13 s,
  OnExplodeAnim "Explode" 0,27 s). In den Daten explodieren sie NICHT: keine Explosions-/Feuer-
  Effekte, kein HurtRadius; nur die Sockets (Fenster, Scheinwerfer) haben References mit StartTag
  "Destroyed" (KFX_VHC.FX_JiggleCar.WindowExplode_Side / HeadlightExplode,
  A2_Destruction_C.debris.SC_Debris_Glass). Schadens-Schwellen (DamageThresholds Crack / Destroy,
  HandleBoneDamage, PlayExplosionEffects) sind nativ.
  - Umgesetzt: die Autos werden CPU-geskinnt statt in der Referenzpose gezeichnet (main.rs
    Level-Schleife, Kollision unverändert; HB_STATICCARS=1 = alt). Schuss auf ein Auto (impacts.rs
    Impacts::shot -> levelcars::world_hit, Spieler und Bots) spielt OnShotAnim und zerbricht
    Sockets nahe dem Treffer; Explosion nahe dem Auto (Grenades::exploded) spielt OnExplodeAnim und
    zerbricht die Sockets in Reichweite: deren "Destroyed"-Partikel / SoundCues (gepoolt).
  - NICHT aus Daten: Trefferkörper = Hüllkugel des Meshes, Socket-Trefferradius 45 uu,
    Explosions-Reichweite 450 uu, Socket bricht beim ersten Treffer, Knochen des gebrochenen
    Sockets auf 0 skaliert (Glas weg), nur animierte Spuren angewandt (das gemeinsame Truck-AnimSet
    trägt in den konstanten Spuren die Truck-Referenzpose; Auto / Van behalten ihre eigene),
    letzte Animationspose bleibt stehen, DrawScale der Akteure ignoriert, SoundNodeWave-Referenzen
    (Scheinwerfer BulletGlass06) nicht gespielt. Test: HB_CARDBG=1 loggt Treffer / Explosionen.
  - Dazu 103 GCDestructibleStaticObject (Schilder, Straßenlampe, Mülleimer, Strommast,
    DSO_Breakable): nicht umgesetzt.
- Wrack-Anbauteile: vehload.rs attachment_specs_sel(.., wreck true) nimmt die *wreck-
  StaticMeshComponents (wingleftwreck / wingrightwreck, Mirror_*_wreck, attach*wreck) mit ihren
  Materials (*_DEST, als 4. "SM|"-Feld); destruction.rs zeigt sie mit dem Wrack. NICHT aus Daten:
  Socket der Wrack-Teile wie beim intakten Teil (PostBeginPlay-Zuordnung, sonst Attach_Socket),
  Element k = Section k.
- Dev-Tools: crates/upk/examples/sockfx.rs (Socket-References eines Meshes mit Start-/EndTag),
  ragdefs.rs (RagdollDefinitions einer Klasse mit Events / Materialien), animdump.rs (Schlüssel
  einer AnimSet-Sequenz).

## Tageszeit + Wetter (2026-10-21, Nutzerwunsch; NICHT im Originalspiel, standardmäßig AUS)
- Neu: `crates/homebots/src/weather.rs` (WeatherPlugin, eine Zeile in main.rs). Aus = exakt der
  bisherige Look (keine Puffer-Schreibzugriffe, keine Entities, Shader-Zweige mit Nullwerten neutral).
- Einschalten: `HB_WEATHER=clear|rain|storm|snow|fog`, `HB_DAYNIGHT=<Minuten pro 24 h>`,
  `HB_TIME=<Stunde 0-24>` (Start; ohne = die Sonnenstellung des Levels), `HB_VOLUMETRIC=1`
  (Lichtstrahlen). Debug-Menü F1 > Werkzeuge > "Tageszeit / Wetter": an/aus, Wetterart,
  Uhrzeit-Regler (+ "Level-Zeit"), Tageslänge, Lichtstrahlen, "Blitz jetzt".
- Crate/Repo: keins von crates.io. Gesucht: Bevy 0.16 Atmosphere/VolumetricFog (eingebaut),
  bevy_atmosphere, bevy_hanabi, Wetter-Crates. Die Level werden nicht mit Bevys PBR-Licht
  gezeichnet (GameMat/TerrainEmu/Game-Shader lesen die UE3-Konstanten aus HfGlobals), daher
  würden Atmosphere/Partikel-Crates am Bild nichts ändern. Benutzt wird nur Bevys eingebautes
  `VolumetricFog` (bevy_pbr 0.16) für optionale Lichtstrahlen: es raymarcht gegen die Schatten-
  karten der DirectionalLight, die sunshadow.rs ohnehin füllt.
- Technik: die Level-Konstanten (upper/lower sky, LightColor, LightDirection, Ambient; beim
  Game-Shader-Puffer auch MaterialUpper/LowerSkyColor) werden jedes Frame aus den unveränderten
  CPU-Daten des Puffers berechnet und per queue.write_buffer überschrieben (wie crater.rs).
  gamemat::Globals hat zwei neue vec4 (sky_fx, sky_tint) nach misc (WGSL in gamemat.rs,
  gameshader.rs, terrainemu.rs angepasst). Sky-Flag in objects[].z (main.rs: GCSkyboxActor = 1,
  FogDome = 2) -> gamemat.rs `hf_weather_sky` dunkelt/färbt Himmelskuppel und Nebelkuppel,
  Sterne, Blitz-Aufhellung. Die DirectionalLight wird mitgedreht (Schatten folgen), Farbe/
  Beleuchtungsstärke wie spawn_lighting. Gras (feste Lichtkonstanten) bekommt einen
  Licht-Multiplikator (GrassParams.weather). Nebel: Vollbild-Pass nach dem Opaque-Pass
  (Tiefenpuffer, exponentieller Höhennebel analytisch, Sonnen-Streulicht), vor der Szenenkopie.
  Regen/Schnee: feste Quad-Meshes (2400 Regen-Schleier / 6000 Flocken), Animation komplett im
  Vertex-Shader, kameranahes Volumen mit Wrap (keine CPU-Arbeit pro Partikel).
- Spiel-Assets: KFX_Impacts Textures.Imp_FallingDrops (Regen, 4 Tropfen-Cluster als Schleier),
  KFX_Atmosphere Textures.PCloudDot (Schneeflocke), KFX_Atmosphere Storm.FX_Lightning
  (Himmelsleuchten, im Effekt-Pool, alle 15 s = EmitterDuration über der Kamera neu gestartet),
  Donner = A2_Explosions_S Ambient.AmbientLOD_03/05/08/12/16/20 (es gibt keinen Donner in den
  Spieldaten; ferne Explosions-Rumpler, tiefer abgespielt), Sturmwind =
  A2_AmbientAudio_S_2_oldFL1.Wind.S_Fencewind1_mono (Crossroads/Overpass-Audio-Sublevel).
  Nicht gefunden: Regen-/Schnee-Partikelsysteme, Regen-Sound, Donner.
- NOT from data (alles, Konstanten oben in weather.rs): Sonnenbahn (Tagundnachtgleiche,
  Mittagshöhe max(60°, Level+8,6°), durch die Level-Sonnenrichtung), Licht-Mindesthöhe
  sin 0,12, Sonnen-Warmton (1, 0,52, 0,26), Mond 7 % blau (0,55, 0,65, 1), Himmelslicht nachts
  6 % + Tönung Nacht/Dämmerung, Himmelskuppel nachts 2,5 %, Sterne (Raster 220, Schwelle
  0,992), Bewölkung (Sonne -85 %, Himmelslicht -35 % und 70 % entsättigt, Kuppel -55 %), Schnee
  (Himmelslicht oben x1,15-1,3, unten x1,6-1,9), Übergang 8 s, Wetter-Presets (Bewölkung,
  Regen, Schnee, Nebeldichte 0,0025/0,004/0,006/0,012 pro m, Wind 3/1,2 m/s), Blitz (alle
  5-16 s, 0,4-4 km, 3 Pulse, Licht 9x, Himmel 2,5x, Ambient 0,35, Licht aus Blitzrichtung),
  Donner-Verzögerung Entfernung/343 m/s, Tonhöhe 0,6-0,8, Lautstärke nach Entfernung, Wind 0,45,
  Nebel (Farbe 0,9 x Himmelslicht + 0,03 x Licht, Höhenabfall 0,01/m, max 97 %, Himmel 3 km,
  Sonnenkeule mu^8 x 0,15), Regen (Volumen 36x16 m, Schleier 1,6x3,2 m, 9 m/s, Deckkraft 0,55),
  Schnee (30x14 m, 4,5 cm, 1,1 m/s, Pendeln 0,35 m), VolumetricFog (Box 1200x300 m, Dichte
  0,0015, 32 Schritte).
- Offen: Regen/Schnee fallen auch unter Dächern (keine Verdeckung), keine nassen Oberflächen/
  Pfützen/Schneedecke, kein Regen-Sound, Nebel wirkt nicht auf transluzente Partikel/Gras, unlit
  Partikel (Rauch) leuchten nachts weiter, Bevy-VolumetricFog-Helligkeit ungetestet. Nicht im
  Spiel getestet (nur gebaut).

## Nächste Karte: Overpass ("HIGHWAY") – nur Team-Deathmatch (2026-10-21, Wunsch "next map")
- Befund aus den Daten: Overpass hat KEINE Bodenkontrolle-Daten. FL-Overpass.map streamt
  tdm_Overpass + Overpass_bcsettings (+ 18 persistente Sublevels), es gibt kein
  Overpass_FrontlinesData, FL-Overpass hat nur PlayerSpawnManagerTDM_Content. tdm_Overpass:
  GameMode_TDMFrontline_0 (keine Ziele, 29+1 Infanterie- und Fahrzeug-Starts in EINER
  SpawnCollection), 48 GameMode_PlayerStarts (Group alt_tdm_infantry / leer), Fahrzeug-Content
  (GCV_US_*/GCV_CH_*), TDM_Settings_Content, 2 GameMode_CombatVolume. Overpass fehlt in
  GCUIInfos GCUIInfo.m_MPMapsInfo; Name/Text ui_frontend.MapSelect.Overpass_Name/_Text
  ("HIGHWAY"), Bild UI_FrontEndTextures_DLC2.MPLoading.Overpass (DLC-Karte).
- Neu mapmode.rs: Spielmodus je Kartenordner. FL = alle gestreamten Sublevels außer TDM_*
  (wie bisher), TDM = alle außer *_FrontlinesData. menu.rs "Starten" nimmt match_maps(folder, mode).
- creategame.rs: "spielbar" je gewähltem Spieltyp (FL: *_FrontlinesData da, TDM: tdm_* da),
  neu berechnet beim Klick auf den Spieltyp (Warteschlange behält nur spielbare Karten).
  Overpass als Zusatz-Eintrag (EXTRA_MAPS). NICHT aus Daten: dass der Bildschirm Overpass listet.
  Test-Hooks HB_MODE=TDM / HB_MAP=Overpass (Vorauswahl, mit HOMEBOTS_MENU_LAUNCH).
- objectives.rs: GameMode_TDMFrontline wird gelesen (Frontline.agnostic, Punkte für beide Teams,
  Default__PlayerSpawnManagerTDM bAgnosticSpawnPoints true). line_spawns ohne Ziele -> tdm.rs.
  TDM-Runde: wer zuerst das Punktelimit erreicht, gewinnt (eine Runde; gcgame
  GCUIScene_MissionBriefing TDMWinningPoints). Punkte wie bisher über KillGameScore je Kill.
- Neu tdm.rs: Spawnwahl. Gewichte aus GCGame.u: FriendlySpawnScore 4, CharacterInfluenceRadius
  6000 (PlayerSpawnManagerTDM), EnemySpawnScore -10 (geerbt von PlayerSpawnManagerMP).
  NICHT aus Daten: Formel (Summe der Gewichte der Lebenden im Radius, bester Wert gewinnt) und
  der Matchstart (noch niemand lebt: jedes Team die Hälfte der Punkte entlang der zwei am
  weitesten entfernten Punkte, USA die erste Hälfte).
- Kreuzung Bodenkontrolle unverändert (gleiche 17 Sublevels wie im Log vom 19.10., 9 Ziele,
  3 Linien, 17/17 Infanterie-Starts der mittleren Linie). Kreuzung mit Spieltyp TDM lädt jetzt
  TDM_Crossroads_c statt der Bodenkontrolle-Daten (vorher lief dort trotzdem Bodenkontrolle).
- Neu HB_MAPCHECK=<Ordner>[:<Modus>] (mapcheck.rs): Kartendaten ohne Fenster (Sublevels, Ziele,
  Fronten, Starts je Team, Gelände, Minimap-Grenzen, Fahrzeug-Meshes in den Paketen).
  Overpass:TDM: 18 Sublevels, 1 Terrain, Grenzen [-4991,-8906 .. 4771,5647], 30 Infanterie-Starts
  (Start 15/15), Fahrzeug-/Heli-Starts, alle 5 Fahrzeug-Meshes gefunden; Overpass:FL nicht
  spielbar (0 Starts).
- Offen / NICHT getestet: Overpass nie im Fenster gestartet (nur gebaut + HB_MAPCHECK). Nicht
  geprüft: Rendering/Lightmaps (Ordner "lightmaps" klein geschrieben), Navigation der Bots auf
  der Karte, Kollision der Brücken, Minimap-Bild. TDM-Regeln nur teilweise: KP-Belohnung je Kill
  (gcbattlepoints TDM PSR_KillAsInfantry 160) nicht übernommen (combat.rs 20 + Serie), TDM-
  Punktelimit-Bereich (GCOnlineServerOptions m_TDMScoreLimit 9000, 1000-30000) nicht im Regler,
  TerritoryPlane (Werte wirken uninitialisiert) unbenutzt, keine Radar-Sweeps (FL2GameTDM).
  PA-Masten (pa.rs) nur auf Kreuzung. Bots ohne lebende Gegner stehen (goal_for "hunt").
- Nachtrag (2026-10-21, Rückfrage Hauptsitzung):
  - TDM-Kill-Belohnung aus Daten: Coalesced gcbattlepoints [TDM.Infantry] Rewards
    PSR_KillAsInfantry Points=160 (tdm.rs kill_bp, von objectives.rs beim Laden gesetzt, beim
    Verlassen zurückgesetzt); combat.rs nimmt sie im TDM statt KillBP 20 der Serie. NICHT aus
    Daten: dass dieselben 160 auch die Team-Punkte im TDM sind (statt KillGameScore 10; passt zum
    Limit 9000 = ~56 Kills). Bodenkontrolle unverändert (die Daten nennen dort FL.Infantry
    PSR_KillAsInfantry 130, HomeBots gibt weiter 20 – nicht angefasst).
  - Punktelimit-Regler im TDM: GCOnlineServerOptions m_TDMScoreLimit (CurrentVal 9000,
    1000-30000, Schritt 1000); Bodenkontrolle behält den Bereich der Szene.
  - Bots im TDM ohne lebenden Gegner: laufen zu einem der 5 entferntesten TDM-Spawnpunkte (nach
    Bot-Index) statt stehen zu bleiben (NOT from data).
  - Spieltyp auf TDM: die Warteschlange beginnt neu mit einer reinen TDM-Karte (Overpass/HIGHWAY,
    steht alphabetisch ohnehin vor KREUZUNG), also direkt "Starten" (NOT from data: Regeln der
    Warteschlange). Zurück auf Bodenkontrolle: ausgewählte spielbare Karte bzw. Kreuzung.
  - HB_MAPCHECK druckt zusätzlich TDM-Belohnung und Punktelimit-Bereich aus dem ini.

## Weißer Himmel in den Tests (2026-10-21, 01:00-02:40)
- Ursache: destruction.rs render_wrecks markierte die Wrack-Slots als "versteckt", bevor ihre
  Mesh-Entities indiziert waren (MeshEntities folgt einen Frame später) -> die Wrack-Meshes
  (DEST-Materialien mit Glut-Emissive) blieben am Ursprung in UE-Einheiten (100x groß)
  sichtbar und überstrahlten Himmel und Bäume. Jetzt gilt der Zustand erst, wenn die Entities
  gefunden wurden. Test e6: Himmel wieder normal.
- Nebenbei (Bisektion): weather::sky_flag hat einen Schalter HB_NOSKYFLAG=1; der Blitz-Effekt
  (FX_Lightning) wird nur mit HB_WEATHER (oder HB_LIGHTNINGPOOL=1) gepoolt -> im Debug-Menü
  eingeschaltetes Gewitter hat ohne HB_WEATHER keinen Himmels-Glow; HB_NOWRECKFX=1 lässt die
  Wrack-/Auto-Effekte aus dem Pool. Spiel-Shader-Objektblock: TerrainHeight.z bleibt 0.
- Map-Autos (levelcars.rs) skinned nur mit HB_SKINCARS=1: im Test stand ein Auto hochkant
  (Orientierung des Skinning-Pfads noch falsch); Standard sind wieder die Referenz-Meshes.
- Wrack-Mesh: im Test (LAV, Gewitter, HB_TEST_VEHKILL) leuchtete das Wrack als große
  überstrahlte Fläche (DEST-Material-Emissive vermutlich falsch ausgewertet). Bis zur Klärung
  wird das Wrack-Mesh nur mit HB_WRECKMESH=1 gezeichnet; Explosion, Feuer, Sounds, Splash
  bleiben. Regen/Gewitter liefen im selben Test sichtbar.

## Raketen gegen Fahrzeuge, Heli ohne Pilot (2026-10-21 Mittag)
- Fahrzeuge hatten keine Trefferfläche: Raketen flogen durch, und Schaden bekam nur ein
  Fahrzeug, in dem der Spieler saß (seine Treffer wurden weitergereicht). Jetzt: vehicles.rs
  VehicleVolumes, pro Fahrzeug eine orientierte Box (half_len / half_width aus dem Mesh bzw.
  Heli-Rumpf, Boden = Ursprung - ride_height bis zum höchsten Mesh-Vertex `top`). Raketen
  (combat.rs update_grenades) explodieren am ersten Treffer von Welt, Soldat oder Box (eigene
  Raketen ignorieren das Fahrzeug des Spielers). Explosionen treffen Fahrzeuge im Radius
  (Abstand zur Box, gleiche Falloff-Regel wie bei Soldaten); vehicles.rs empty_vehicle_damage
  rechnet wie im Spielerfahrzeug über das Damage-Grid auf die Panzerung um und zerstört bei 0
  (Explosion + Wrack wie gehabt). NOT from data: Box statt PhysX-Körper.
- Heli ohne Pilot (in der Luft ausgestiegen): fällt mit HELI_G, verliert langsam Fahrt, kippt
  (NOT from data: Kipprate), schlägt auf; schneller als HELI_CRASH_SPEED 600 uu/s (NOT from
  data) = zerstört. Am Boden bleibt er stehen.
- Offen: Infanterie-Kugeln und Fahrzeugkanonen gegen leere Fahrzeuge (gehen noch hindurch).
- Performance (Profiler-Log Angel Island: render_vehicles 2,2 ms/Frame, allocate_and_free_meshes
  3,3 ms): geparkte Fahrzeuge werden nicht mehr jeden Frame CPU-gehäutet; neu gehäutet nur wenn
  sich Position/Lage/Türme/Räder/Rotoren oder die Ansicht ändern (erst nachdem die Mesh-Entities
  indiziert sind).

## Angel Island: Schachbrett-Straßen, keine KVA-Bots (2026-10-21, Nutzertest "Angel Island")
- Karte = Maps/MP/AngelIsland (Bodenkontrolle), nicht Overpass. Dateien vom PC des Nutzers in
  /mnt/user-data/uploads gestaged (AngelIsland-Ordner, Statics_FL2/Roads.upk), nicht ins Repo.
- Keine KVA-Bots: objectives.rs load_frontlines ordnete jede SpawnCollection über die Group ihrer
  PlayerStarts einem Team zu (*_High = USA, *_Low = KVA). Auf Angel Island haben alle 198 Starts
  Group "" -> beide Sammlungen landeten bei USA, KVA hatte 0 Starts (Log des Nutzers: "front
  lines: 3 ([[18, 0], [18, 0], [17, 0]] USA / KVA infantry starts)"), bots.rs pick_start fand für
  KVA nichts. Jetzt aus Daten: SpawnPointCollection.Team (Struct-Default TEAM_RedStar,
  Default__GameMode_Frontline SpawnCollection[1] Team TEAM_Western; Core.u ETeam 0 TEAM_RedStar =
  KVA, 1 TEAM_Western = USA), also SpawnCollection[0] = KVA, [1] = USA. Gegenprobe Kreuzung:
  Group-Regel und Team stimmen in allen 6 Sammlungen überein (HB_MAPCHECK "Group/Team mismatches
  0"), Starts unverändert 17/17. Angel Island jetzt USA 18 / KVA 24 (mittlere Linie).
- Schachbrett auf Straßen: kein fehlendes Paket. 31 RoadSplines (30 in Angel_Ter_Terrain, 1 in
  fl-angelisland) mit Material M_DirtRoad (RoadTexture / TerrainOverlay =
  EngineMaterials.DefaultDiffuse, das magenta/olive Karomuster) sind HiddenGame (Actor und
  Komponente, dazu DisableCollision) - das Spiel zeichnet sie nicht. upk level.rs
  road_spline_meshes überspringt jetzt HiddenGame (Actor/Komponente) und nicht platzierte
  RoadSplines (Prefab-Vorlage Roads.Intersections.TJunction_1`0x lag am Weltursprung; gleiche
  Regel "TheWorld.PersistentLevel." wie bei GCDestructibleStaticObject). Angel Island: 69 von 101
  Straßenstücken gezeichnet, alle mit echten Texturen (asphalt_road_Angel*, Roads.upk).
  Kreuzung/Overpass unverändert 8.
- HB_MAPCHECK druckt neu: gezeichnete Straßenstücke und je Material die gebundenen Texturen
  (Markierung bei EngineMaterials.Default*), "Group/Team mismatches" je Front.
- Neues Dev-Tool crates/upk/examples/roadactors (RoadSpline-Actors, HiddenGame, Materialien).
- Nichts Neues ohne Datenquelle. Offen: im Fenster nicht getestet (nur HB_MAPCHECK); Angel
  Islands lightmaps/*_sl.upk werden nicht geladen (wie bisher, nicht untersucht).
- Kugeln (trace_shot: Spieler, Bots, Fahrzeug- und Drohnenkanonen) bleiben jetzt an den
  Fahrzeug-Boxen hängen (Treffer = Welt-Treffer mit Box-Normale, Standard-Einschlag), ohne
  Schaden am Fahrzeug (Wunsch User). Boxen um den Schussursprung (eigenes Fahrzeug) zählen
  nicht. Spieler im Fahrzeug ist damit gegen Kugeln geschützt.

## Nacht: Skyline, Zielfernrohr-Linsen, Möwen aus dem Boden (2026-10-21, Screenshots Engelsinsel)
- Skyline (San Francisco): angelisland_sky.map platziert SKY_Elements.Angel_Island SanFranBack/
  FrontCard (M_SanFran*Card -> MAT_LIB M_Translucent, MLM_Unlit), Tiburon/Alcatraz-Karten
  (M_Masked, beleuchtet), Schiffe (M_Opaque, beleuchtet). Unlit = blieb nachts weiß.
  weather::sky_flag gibt Meshes aus dem Paket SKY_Elements Flag 3; gamemat.rs
  hf_weather_backdrop dunkelt damit NUR unbeleuchtete Materialien ab (beleuchtete folgen schon
  dem Licht). Helligkeit = Himmelslicht-Anteil der beleuchteten Flächen (Nacht 6 %, Bewölkung
  -35 %, Blitz +0,35) mal Himmelstönung, Bewölkung entsättigt; Wert in sky_tint.w
  (1 - Helligkeit, 0 = aus). NOT from data: Helligkeitsregel.
- Linsen: ATW_PZF3 SKW_ATW_PZF3_1P Slot 5 = ATW_JAV M_Scope_Lens01 (unlit, transluzent). Staub-/
  Öl-/Fresnel-Terme hängen an MaterialTintColor = Vektorparameter GlobalMaterialTintColor
  (Uniform-Ausdruck im Shader-Cache, Default 1,1,1; Name in HOMEFRONT.exe), dazu
  ENV_LEVEL_CUBE_REFLECTION (statische Tag-Cubemap). matcompile: MaterialTintColor ->
  hf_material_tint(), Cube-Samples -> hf_env_cube(); gamemat.rs liefert beide mit derselben
  Wetter-Helligkeit (menu.rs/minimap.rs: neutral). Wetter aus = unverändert.
  NOT from data: dass das Wetter GlobalMaterialTintColor und alle Cube-Reflexionen abdunkelt
  (wirkt auch auf Wasser/Glas mit Cube-Reflexion).
- Möwen: Transformkette geprüft (Ort/Rotation/DrawScale, bUseLocalSpace, Location/Velocity
  uniform min.xyz/max.xyz, UE->Bevy) – stimmt mit UE3 überein. Ursache sind die Daten:
  Angel_FX Emitter_269 (SeaBirds, Z -1141) liegt 2,6 m unter dem Hang (eigene
  TerrainLightingHeight -882, Gelände im 6-m-Umkreis -1073..-830); die Vögel starten 0-4 m über
  dem Emitter und steigen nur 0,4-0,8 m/s. main.rs hebt platzierte Emitter unter der
  Geländeoberfläche auf das Gelände (NOT from data). Laut examples/emitterfit (neu) ist das auf
  Engelsinsel/Kreuzung/Overpass der einzige Emitter unter dem Gelände.
- Nicht im Spiel getestet (nur gebaut, WGSL der Linse/Karte mit naga geprüft).
- matcompile: ComponentMask auf nicht vorhandene Komponenten (ScreenPosition.zw) gab leeres Swizzle -> Shader ungültig (FX_lighting M_LightPlaneWarehouse* auf Angel Island übersprungen); jetzt Nullen (NOT from data).

## Angel Island: schwarze Sicheln an den Straßenrändern (2026-10-21, Screenshot Schnee/Nebel)
- Ursache: die Randstücke der Straßen (Roads.upk M_asphalt_road_Angel_edge / _int_C_edge, Parent
  M_Road_Translucent: BLEND_Translucent, MLM_Unlit, Opacity = RoadGradient.g) werden als
  transluzente Meshes NACH dem Vollbild-Nebel von weather.rs gezeichnet (Nebel-Pass liegt
  zwischen MainOpaquePass und MainTransmissivePass) und bekamen keinen Nebel. Gelände und
  Straßenfläche (opak) verschwinden im Nebel, die Ränder nicht -> dunkle, scheinbar schwebende
  Bögen. Der Pixelshader des Spiels für den Rand (matps, FLocalVertexFactory) hat selbst keinen
  Nebelterm; Geometrie/UVs/Blend der Ränder sind unverändert richtig.
- Fix: gamemat::Globals um `fog` (3 vec4: Farbe+Dichte, Sonnenrichtung+Höhenabfall,
  Sonnenstreuung+Max) erweitert (WGSL-Structs in gamemat/gameshader/terrainemu gleich);
  weather.rs schreibt dieselben Konstanten wie der Vollbild-Pass (translucent_fog, auch von
  apply_fog benutzt) in alle Globals-Puffer, 0 = aus. weather::TRANSLUCENT_FOG_WGSL rechnet den
  Nebel an der eigenen Position der Fläche; gameshader (Spiel-Shader-Pfad) und gamemat
  (Graph-Pfad) nebeln Translucent (zur Nebelfarbe) und Additive (ausblenden), Modulate und
  Himmels-/Nebelkuppel bleiben unverändert. NOT from data: die Wetter-Nebelregel selbst
  (wie bisher) und dass transluzente Flächen sie bekommen.
- HB_MAPCHECK: je Straßenmaterial blend / two-sided / lighting und ob Spiel-Shader- und
  Graph-WGSL validieren, dazu "fogged in shader". Angel Island: 2 Rand-Materialien
  Translucent, beide WGSL ok, im Shader genebelt.
- Nicht im Fenster getestet (nur gebaut + mapcheck). Offen: Modulate-Materialien ohne Nebel.

## Regentropfen auf dem Bildschirm (2026-10-21, Wunsch User "subtle")
- Neu: crates/homebots/src/screenrain.rs + screenrain.wgsl, eingehängt in main.rs (mod +
  Plugin), weather.rs nur neuer Getter `Weather::rain_amount()` (0 bei Wetter aus).
- Datenlage: Homefront hat keinen Regen und keinen Regen-auf-Linse-Effekt. Einziger
  Wasser-Linseneffekt: Kugel-Einschlag im Wasser, KFX_Impacts FX_Aftermath.AFM_Bullet_Water
  Emitter 0 -> Material Shaders_Aftermath.LensFX_Waterdroplets (Translucent, Unlit, Emissive =
  SceneTexture(ScreenPosition + (LensFX_Waterdroplets.g - 0,4) * VertexColor.a), Panner
  SpeedY -0,15, AlphaOverLife 0,1 -> 0 in 1,3 s). Textur = verwischter Spritzer, keine
  Einzeltropfen (dazu LensFX_Watersplash / WaterGlare / DirtDrops: Bokeh/Schmutz). Aus Daten
  übernommen: Methode (Szene an verschobener Bildposition neu abtasten, keine Tönung, keine
  Zusatzfarbe) und die Größenordnung der Verschiebung (Spiel max. 0,06 Bild, hier < 0,03).
  NOT from data: die Tropfen selbst (prozedurale runde Perlen in 2 Rastern, wenige langsam
  rutschende Tropfen mit dünner Nassspur und Mini-Perlen), Größen, Anzahl, Lebensdauer,
  Geschwindigkeit, linsenartige (umgedrehte) Verschiebung, leichte 4-Tap-Unschärfe, Rand -12 %.
- Ein Vollbild-Pass nach hfpost (HfPostLabel -> ScreenRain -> EndMainPassPostProcessing), unter
  der UI-Kamera; ohne sichtbare Tropfen wird der Pass gar nicht ausgeführt. Pixel außerhalb der
  Tropfen = 1 textureLoad.
- Menge = Wetter-Regen (Regen 0,6 / Gewitter 1,0), unter Dach reduziert (3 Strahlen 30 m
  senkrecht nach oben gegen CollisionWorld, Anteil Treffer), aus im geschlossenen Cockpit
  (Sitz in 1. Person mit Cockpit-Mesh, cockpit1p), Einblenden 2,5 s / Ausblenden 4 s; Tropfen
  verschwinden dabei einzeln (Dichteschwelle je Zelle). Alles NOT from data.
- Schalter: HB_SCREENRAIN=0 aus, =1 erzwungen voll (Test, ignoriert Wetter/Dach/Cockpit),
  =0.6 erzwungen mit Menge; Resource ScreenRainSettings { enabled, strength (0,85) }.
- Nicht im Fenster getestet; WGSL mit naga geprüft, Vorschau per numpy-Nachbau auf einem
  Screenshot (Gewitter: ca. 45 Perlen + 7 Rutscher, 0,9 % der Pixel).

## PROX-Werfer: Lock-on, Zielsuche, Annäherungszünder; Leuchtspur-Lichter (2026-10-21)
- Daten: CFG_ATW_PROX hat KEIN LockOn (Annäherungswerfer: "Detonation, sobald die Rakete in der
  Nähe eines feindlichen Fahrzeugs ist"), GCProj_ATW_PROX_C bSeekTarget false,
  Seek_LoseTargetMaxDot 0,45, ProximityVsAnyVehicleRange 50, Speed 6000, DamageRadius 300,
  Damage 100 (GCProjectile), MyDamageType GCDmgType_ATW_PROX; Eltern GCProj_ATW_JAV_C ->
  GCProj_MissileTrackingTopDown (bSeekTarget true, Seek_DelayTime 0,15, Seek_VInterpToValue 1,5).
  Lock-on hat nur CFG_ATW_JAV (AAWS-M): LockOn SupportedPawnClasses GCVehicle, BigRectangle
  -239,-115 .. 239,130,8, SmallRectangle ±40 / Material -40,-49 .. 49,40, LockTime 1,2,
  LockGraceTime 2, LockGraceMaxAngle 90, MaxLockDist 15000, Struct-Defaults MinLockDist 0,
  bAllowUnZoomedLock false, bSkipLineOfSightCheck false.
- weaponcfg: LockOnCfg / read_lock_on (WeaponCfg.lock_on), ProjectileCfg seek/seek_delay/
  seek_interp/seek_lose_dot/prox_vehicle_range, read_projectile_script (GCGame-Elternklassen).
  main.rs: PROX bekommt das LockOn von CFG_ATW_JAV (NOT from data, Wunsch User), Log "lock-on ...".
- combat.rs player_lock_on (vor player_weapon): nur gezoomt (ADS >= 0,95, NOT from data
  Schwelle), Fahrzeug-Box-Mitte im BigRectangle, Distanz, Sichtlinie (Level + erste Fahrzeugbox);
  nach LockTime gesperrt; gesperrtes Ziel außerhalb der Box hält LockGraceTime innerhalb 90°.
  NOT from data: Box-Mitte als Prüfpunkt, Ziel nächst zur Bildmitte, leere Fahrzeuge zählen
  (kein Team-Test), unfertiger Lock außerhalb der Box beginnt neu. GCLockOnTargeter ist nativ,
  nicht dekodiert. HB_LOCKDBG=1 loggt den Zustand.
- Schuss mit Lock: Rakete sucht das Fahrzeug (Grenades.seek parallel zu list): nach
  Seek_DelayTime Flugrichtung VInterpTo(Richtung, zum Ziel, dt, 1,5), verloren unter Dot 0,45.
  Native Seek-Logik nicht dekodiert (Lesart der Namen), Lock nach dem Schuss weg.
- Annäherungszünder (Daten): Rakete zündet 50 uu vor jeder Fahrzeugbox (nicht das eigene).
- HUD (hud.rs WeaponLockHud, an der Infanterie-Placer): großer Rahmen grün, Zielrahmen grün
  blinkend (schneller mit Fortschritt), gesperrt rot. NOT from data: Farben/Linien (M_Lockon-
  Material nicht gerendert). Lock-Sounds fehlen.
- Fahrzeugschaden geprüft, entspricht den Daten: GCProjectile.ProcessTouch -> Explode (DamageRadius
  > 0), getroffenes Fahrzeug volle Wirkung; DamageGridMP GCDmgType_ATW_PROX: PMA_Heavy 250,
  HeavyStrong 170, Medium 300, Light 300, Heli 350, HeliLight 400, DroneHeli 1000, DroneTank 500
  (Soldat 200/98); VehicleDamageScaling 1; Health GCVehicle 1000 (MD600 1200). => M1A3/LAV/
  HUMVEE/AH64 je 4 Volltreffer, laut Daten ist der PROX schwach gegen Panzer (PZF3/ATRL 500 auf
  Heavy = 2 Treffer). Nicht geändert (kein Wert ohne Datenquelle). Nicht modelliert:
  OverArmor-Platten (GCOverArmorInfo DamagePassThrough 0, würden mehr aushalten).
- Leuchtspur: combat::fire_tracer (Spieler, Bots, Fahrzeugkanonen) färbt je Schuss zufällig
  Daten-Gelb / Rot / Grün (NOT from data – user request) und legt ein Licht an den Kopf der
  Leuchtspur (TRACER_LIGHTS, dynlights.rs: die 8 nächsten zur Kamera, Radius 300 uu,
  Helligkeit 4; NOT from data – user request).
- Nur gebaut, nicht im Fenster getestet.

## Fahrzeug-Wracks: leuchtende Fläche behoben, Teile fliegen ab, Map-Autos-Ausrichtung (2026-10-21, Wunsch "Wrackteile / Wreckage")
- Ursache "leuchtende Fläche": die *_DEST- und *_Damage-Materialien (MAT_LIB M_Vehicle_DEST /
  M_Vehicle_Damage) importieren GLO_textures.soot02_C / _M / _N und GLO_textures.Embers; das
  Paket GLO_textures exportiert keine Datei des Uploads (expfind über alle .upk/.map). Ungebunden
  = Bevys weißer Ersatz: Embers = 1 -> Emissive (5, 2, 0,4375) * lerp(1, 25, (PixelDepth/2000)^3)
  auf der ganzen Rußfläche (bis 125-fach überstrahlt ab 20 m), soot02_N = 1 -> jede Normale um
  (3,333, 3,333, 0) verbogen. Fix: world.rs Spawner.tex_substitute (nur während
  destruction::load gesetzt): Embers und soot02_N schwarz (keine Glut, Ruß-Normale neutral);
  soot02_C/_M bleiben weiß (voll verrußt, Rußfarbe 1 * 0,01 = fast schwarz). NOT from data: diese
  Ersatzwerte (die echten Texturen fehlen). Gate HB_WRECKMESH entfernt, Wrack wird immer gezeichnet;
  HB_NOWRECKMESH=1 schaltet es aus. Wrack-Entities bekommen NoFrustumCulling (wie Fahrzeuge).
- Abfliegende Teile aus den Daten: RagdollDefinitions RagdollAsset (PA_*_wreck*): RB_BodySetup
  BoneName, RB_ConstraintSetup ConstraintBone1/2 + bLinearBreakable/bAngularBreakable. Körper mit
  brechbarem Gelenk (LinearBreakThreshold 10) oder ohne Gelenk = eigenes Teil; per unbrechbarem
  Gelenk verbundene Körper gehen mit (M1A3 wreck0: Turm TURRET_ROOT+Turret_Rotate und Rohr
  TURRET_BARREL; M1114 wreck2/4/5: MG_ROTATE + 2 Räder; T99: Turm; LAV Wreck0: Turm + WHL_L1/
  WHL_R3, Wreck1: Turm; AH64 TailBreak usw. je nach Asset). Knochen gehören zum nächsten Körper
  (selbst oder Vorfahr). Teile werden mit eigener Lage gehäutet (destruction.rs wreck_pose), ihre
  Socket-Effekte (Feuer, Rauch, Sounds) wandern mit dem Teil (bone_frame).
  NOT from data: Abwurf (PIECE_UP 450, PIECE_OUT 250 uu/s, Spin 2,5 rad/s, zufällig gestreut),
  Starrkörper ohne Gelenkgrenzen, Bodenkontakt über den tiefsten Vertex des Teils (Referenzpose,
  ohne Drehung), Abprall/Reibung wie das Wrack, Bruch sofort beim Tod (PhysX bricht bei Schwelle 10
  praktisch sofort). Nicht umgesetzt: M1114-Kismet GCSeqEvent_RigidBodyCollisionWithEffect
  (MinCollisionVelocity 250, MaxTriggerCount 2) -> GCSeqAct_BreakConstraint WHL_R1/WHL_L0.
- Map-Autos (levelcars.rs): Ursache gefunden – skin_part_scaled bildet Mesh-Raum fest mit
  (m.z, -m.x, -m.y) ab; das ist genau UE3 FRotationMatrix des Fahrzeug-RotOrigin (0, -16384, 16384)
  (Unit-Test levelcars::rot_tests). Die DEST_*-Autos haben RotOrigin (0, 32768, 16384) (Höhe =
  -y laut Vertex-Grenzen). levelcars nimmt jetzt RotOrigin/Origin des Meshes (wie der
  Referenzpfad in main.rs) und hebt die feste Abbildung auf. Gate HB_SKINCARS bleibt (nicht im
  Fenster geprüft; offen: ob die Animationsspuren des gemeinsamen Truck-AnimSets den Root drehen).
- Neue Dev-Tools: crates/upk/examples/wreckmat.rs (MIC mit fremdem Parent kompilieren, Texturliste),
  wreckpa.rs (Körper/Gelenke von PhysicsAssets), skorigin.rs (Origin/RotOrigin + Vertex-Grenzen).
- Nicht im Spiel getestet (nur gebaut + Unit-Test). Zu prüfen: HB_TEST_VEHKILL=3 mit M1A3/LAV
  (Turm fliegt ab, Wrack dunkel verrußt, keine Überstrahlung), HB_SKINCARS=1 (Autos stehen richtig).

## Büsche mit Kollision, Bot-Wege Angel Island (2026-10-21, Bot-Log 1792581662 + Wunsch "Bushes have a collision box")
- Ursache Büsche: unser Kollisionsbau gab jedem StaticMesh mit Komponenten-Flags (CollideActors/
  BlockActors/BlockNonZeroExtent) seine Render-Dreiecke als Spieler-Kollision. Die Regel des
  Spiels steht in HOMEFRONT.exe UStaticMeshComponent::LineCheck (0x583dbe): Nicht-Null-Extent
  (Spieler) + UStaticMesh.UseSimpleBoxCollision (+0x11c) bzw. Null-Extent (Schüsse) +
  UseSimpleLineCollision (+0x118) -> nur die vereinfachte Kollision (BodySetup +0xd0, sonst +0xd8,
  AggGeom); ohne BodySetup "kein Treffer" (0x5841f2 -> 0x584821), sonst per-Poly-kDOP.
  Defaults aus den Daten (serialisiert wird nur der Nicht-Default: UseSimpleBoxCollision=0,
  UseSimpleLineCollision=1): Box TRUE, Line FALSE. Neu upk mesh.rs
  static_mesh_collision_mask (Bit 0 Spieler, Bit 1 Schüsse), main.rs verknüpft es mit den
  Komponenten-Flags; Log je Paket "placements without pawn collision". Dev: HB_MESHCOL_RENDER=1 =
  alte Regel. Geometrie mit BodySetup bleiben die Render-Dreiecke (NOT from data: statt der
  Konvex-Hüllen).
- Angel Island (examples/meshcolsurvey, neu): 879 Platzierungen / 49 Meshes verlieren die
  Spieler-Kollision, Schüsse treffen sie weiter (außer Pfützen mit UseSimpleLineCollision=1).
  Büsche STN_undergrowth grass_Long_03 (171) und weed_Cracks (7) – aber laut Exe-Regel auch
  Felsen Boulder8 (232) / Boulder1 (86) / cliff_convex_02/03 (60), Barrack-/Hospital-Fenster,
  barricadeWallMetal05 (28), Kisten, Veranden (Farm_Porch, Barrack_Porch), Hintergrund-Gebäude
  (Barrack_Building, Hospital_Building_A/B, Warehouse_A_Combined_A, Comm_Building), Skybox-Karten.
  Alle haben kein BodySetup; begehbare Böden/Treppen/Wege haben alle eins (Farm1_Floor_BR ohne
  BodySetup hat UseSimpleBoxCollision=0 gesetzt) – passt zur Regel. Die Gebäude liegen fast alle
  außerhalb der Bot-Wege (im Log nur 2 Platzierungen < 5 m von Bot-Positionen). NICHT geprüft:
  ob +0xd8 zur Laufzeit gefüllt wird (Schreiber nicht gefunden) – falls Felsen im Original
  blockieren, ist das die Stelle.
- Bot-Log (Angel Island, 649 s, 15 Bots): stuck 24, stalled 24 (3x "third time"), trapped 5, no
  progress 13, "NO nav path" 104 (alle in den ersten 6,5 s). Die Wiederholungs-Stellen: (-41,-29)
  7x und (-15,-37) 5x direkt auf grass_Long_03 (0,3/0,5 m), (-31.7,-46.1) 5x an Farm_Porch,
  (-15,-40) 3x grass_Long_03, (-18,13) barricadeWallMetal05. 16 (+7 an (-41,-29), dort auch ein
  Mast) der 66 Hänger-Ereignisse liegen an Meshes, die jetzt nicht mehr blockieren.
- Walk-Grid (nav.rs) baut jetzt schon, sobald die Kollision der Karte steht (Creating/Loading/
  Movie statt erst ab Spawn): ~5 s früher fertig (Loading 25,7 s -> Playing 30,5 s im Log), die
  6,5 s Luftlinie am Rundenstart schrumpfen auf ~2 s. Das Grid nutzt dieselbe Spieler-Kollision,
  Büsche sind damit auch keine Hindernisse/Inseln mehr.
- Nicht im Spiel getestet (gebaut, Survey offline). Zu prüfen: Angel Island durch die Büsche
  laufen; Log-Zeile "placements without pawn collision"; nächstes Bot-Log (Stellen oben).

## Regenbogen-Pfeiler, Nachtlicht, glänzende Waffe (2026-10-21 14:45, Screenshots Engelsinsel)
- Regenbogen-Pfeiler (Tor-Pfosten, GLO_Fences / STO_barriers indGateHalf, Material
  GLO_Fences.M_indGateNonDEST_C): Ursache matcompile – der Homefront-Knoten
  MaterialExpressionRMapSample war ein Stub, der seinen ersten Eingang durchreichte = die
  UV-Koordinaten (TextureCoordinate_1) als Farbe -> Rot/Grün-Verlauf. Aus Daten: der Knoten hat
  Texture (GLO.white_C) + Coordinates; der gecachte PS (FLocalVertexFactory, NoLightMap SkyLight)
  sampelt Texture2D_1 an UV-Satz 2 (v0.wz) und multipliziert ihn in Diffuse/Specular. Jetzt wie
  TextureSample kompiliert. Betrifft 58 Materialien auf Engelsinsel/Kreuzung/Overpass
  (examples/matsuspect).
- Schwarze Dreiecke am Boden (71ffb676): sunshadow.rs nutzte für ALLE Karten die auf Overpass
  gemessene "Full Level"-Schattenkugel (Zentrum fest, r 20128); außerhalb wurde die Randzeile der
  Shadow-Map über das Gelände gezogen. Jetzt (NOT from data): Kugel mit dem gemessenen Radius um
  die Kamera, Punkte außerhalb einer Kaskade gelten als beleuchtet.
- Nacht (weather.rs, NOT from data): Mond 0,07 -> 0,035 der Sonnen-Luminanz, Himmelslicht nachts
  0,06 -> 0,12. Vorher hatte die Nacht den Tag-Kontrast Sonne:Himmel (~5:1); mit der Augen-
  Anpassung des Spiels (HDRLumHighClamp 3,5) sahen mondbeschienene Flächen wie Tag aus, alles im
  Schatten (Häuserwände) schwarz. Jetzt ~1,5:1. Rückwirkung: Skyline-Karten/Env-Cubes folgen
  (hf_weather_dim nutzt denselben Anteil) und sind nachts 12 % statt 6 %.
- Waffe/Hände nachts zu glänzend: 1P-Meshes haben eine gestauchte Tiefe (gamemat.rs), der
  Schatten wurde aus clip.z rekonstruiert -> falscher Ort, Waffe immer voll vom Mond/der Sonne
  beleuchtet (auch in Häusern, inkl. Specular). Jetzt Schatten an der echten Weltposition
  (sunshadow hf_sun_shadow_at, VOut.fp). NOT from data: UE3 beleuchtet 1P über eine
  Light-Environment.
- Leuchtendes Garagentor (c3013cb8): nicht gefunden. FX_lighting M_LightPlaneWarehouseBright ist
  Modulate mit Clamp(.., Min 1, Max 1) = 1, also unsichtbar – nicht die Ursache.
- Neue Dev-Tools: examples/matsuspect, meshmatall, expidx, matcube, placefind.
- Getestet: gebaut. Im Fenster NICHT verifiziert (3 Läufe: 2x OOM wegen paralleler Builds, 1x
  kamen die Screenshots nicht mehr vor dem Beenden an). Vorher-Bilder:
  outputs/lighting-1445/.

## Nachtrag zu "Büsche mit Kollision, Bot-Wege Angel Island" (2026-10-21)
- Gegenprobe Crossroads/Overpass – die Exe-Regel nähme dort auch den
  intakten Autobahn-Decks HW_2Lane_2way_Traffic (24 Platzierungen, kein BodySetup, kaum
  BlockingVolumes darunter; Bots standen im alten Log 2,5-5 m darunter) die Spieler-Kollision,
  dazu Boulder/Barrikaden. Weil nicht geklärt ist, ob +0xd8 zur Laufzeit gefüllt wird, gilt die
  Regel standardmäßig NUR für Meshes, deren Sektionen alle ein Vegetations-PhysicalMaterial haben
  (PM_Library.Env.Wood PM_Vegetation / PM_LeavesDeciduous; NOT from data: diese Einschränkung).
  HB_MESHCOL=exe = Regel für alle Meshes, HB_MESHCOL_RENDER=1 = alte Regel. Offline (meshcolsurvey
  Spalte VEG): Angel Island 178 Platzierungen nicht mehr blockierend = grass_Long_03 171 +
  weed_Cracks 7; Crossroads und Overpass je 5 (weed_Cracks 4, leavesScattered02 1). Felsen,
  Wände, Decks unverändert. Kein Vegetations-Mesh mit BodySetup auf Angel Island/Crossroads.
  Die Zeile im Spiel-Log heißt "... placements without pawn collision (simple collision, no
  BodySetup, vegetation)".

## Kollisionsregel geklärt: Homefronts Kollisions-Mesh; Bot-"no progress" auf Angel Island (2026-10-21)
- Die "BodySetup +0xd0"-Deutung im Abschnitt "Büsche mit Kollision" war falsch. HOMEFRONT.exe
  UStaticMeshComponent::LineCheck (0x583d50): mit UseSimpleBoxCollision (Spieler) bzw.
  UseSimpleLineCollision (Schüsse) und ohne Trace-Flag 0x20000 prüft sie ZUERST einen eigenen
  kDOP-Baum (UStaticMesh+0xcc Zeiger-Array, Anzahl +0xd0, 0x583de6 -> 0x5911d0/0xd505d0/
  0x9d6230). Dessen Vertices/Materialien kommen aus (*(+0x48))[0] +0x50/+0xf8 – gleiche Offsets
  wie ein FStaticMeshRenderData der normalen LODs (+0x3c, kDOP je LOD +0xbc[LODForCollision
  +0x7c]). Also ein zweites LOD-Array = Kollisions-Mesh. Nur wenn es fehlt: BodySetup (+0xd8,
  AggGeom +0x48, PhysMaterial +0xc4, 0x5841f2); ohne beides kein Treffer.
- In den Daten: nach den LOD-Bulkdaten 8 Dwords (LOD-Anzahl zweimal, ThumbnailAngle/-Distance,
  2x 0), dann die Anzahl der Kollisions-Meshes, je eines wie ein LOD (Sektionen, Positionen
  Stride 12, ..., kDOP-Knoten, Dreiecke 8 Byte v0 v1 v2 Material). Boulder8: 622 Dreiecke (Render
  LOD0 1364), Farm_Porch 76, barricadeWallMetal05 36, HW_2Lane_2way_Traffic 32;
  grass_Long_03/weed_Cracks: 0. Neu upk mesh.rs static_mesh_tail /
  static_mesh_collision_model(_count) / static_mesh_pawn_collision; ~18 Meshes je Karte mit
  anderem LOD-Layout (DEST_tire01, milCrate03, indFence4, ...) werden nicht dekodiert und
  blockieren weiter (NOT from data; alle haben BodySetup oder UseSimpleBoxCollision=0).
- main.rs: Spieler kollidieren jetzt mit den Dreiecken des Kollisions-Meshes (statt Render-
  Dreiecken), PhysMaterial der ersten Sektion (NOT from data). Schüsse weiter Render-Dreiecke
  (exe: kDOP des Render-LODs, bei UseSimpleLineCollision das Kollisions-Mesh – Pfützen; hier
  NOT from data die Render-Dreiecke). Mit BodySetup ohne Kollisions-Mesh weiter Render-Dreiecke
  statt Konvex-Hüllen (NOT from data). Vegetations-Sonderfall und HB_MESHCOL=exe entfernt –
  die Exe-Regel liefert es von selbst; HB_MESHCOL_RENDER=1 bleibt (alte Regel).
- Ohne Spieler-Kollision (examples/meshcolsurvey): Angel Island 215 Platzierungen = grass_Long_03
  171, weed_Cracks 7, Sidewalk_Urb_Broken_Transition 14, hangingTarp01 9, Skybox-Karten/Schiffe 11,
  Graffiti 2. Crossroads 91 / Overpass 88: Decals, Graffiti, Kabel, rebarIndustrial,
  HWsignScaffolding2, Schilder, weed_Cracks, leavesScattered02. Felsen, Klippen, Wände,
  Veranden, Gebäude, Autobahn-Decks blockieren wieder.
- Bot-Log 1792581662, "no progress" 3x bei (18 -10 38) (Ziel C): kein Hänger – Spieler5 lief
  konstant ~2 m/s, nur führte der Nav-Pfad vom USA-Spawn erst weg von C (Luftlinie 67 -> 100 m
  in 15 s). Der Wächter maß die Luftlinie. bots.rs misst jetzt die Restlänge des geplanten Pfads
  (+ Pfadende -> Ziel). Die Stelle liegt in einer Lücke zwischen zwei Boulder8.
- Farm_Porch (-31.7 -46.1) und barricadeWallMetal05 (-18 13) haben Kollisions-Meshes (76/36
  Dreiecke); Spieler und Walk-Grid nehmen jetzt diese statt der Render-Dreiecke (Pfosten,
  Stufenkanten). Ob das die Hänger dort behebt: NICHT geprüft (Walk-Grid offline nicht gebaut,
  der Kollisionsaufbau hängt am Bevy-Ladepfad). Neue Dev-Tools: examples/coltail, placenear.
- Getestet: gebaut, Survey offline. Im Spiel NICHT getestet: Felsen/Decks begehbar und
  blockierend, Büsche durchlaufbar, Log-Zeile "placements without pawn collision (simple
  collision, no collision mesh, no BodySetup)", nächstes Bot-Log.

## Leuchtendes Garagentor = Gottesstrahl-Emitter, Partikel/Unlit nachts dunkler (2026-10-21, Screenshot c3013cb8)
- Ursache Tor (Engelsinsel, Scheune Farm1_Extension bei Ziel A / Command, Anhänger semiBed_Frame
  (-270, 5462)): kein Mesh, sondern der platzierte Emitter Angel_FX Emitter_52 (-512, 5808, -1440)
  mit KFX_Atmosphere.FX_Godrays.GRay_Beams_B (6 Sprites, Shaders.GRay_Test, MLM_Unlit Additive,
  Ausgabe = Opazität * (Textur + Partikelfarbe), cooked PS gegengeprüft). Absolut ~0,016 * Tex²
  hell: am Tag 3 % Zugabe, nachts (Mond 0,035) heller als alles andere -> beiger Kasten in der
  Toröffnung. Daneben Dust_swarm (Emitter_185, Shaders.Dust Additive).
- Aus Daten: alle 10 GRay_Beams_B-Komponenten haben MaxDrawDistance 800 (CachedMaxDrawDistance
  1000000 bei allen 49 Angel_FX-Emittern). Neu: upk EmitterPlacement.max_draw_distance = kleinerer
  Wert ungleich 0 der beiden (UE3-Regel "min von LD-Wert und Volumes", im Exe NICHT geprüft);
  particles.rs blendet ein platziertes System aus, wenn der Mittelpunkt seiner Partikel weiter
  als das von der Kamera ist (UE3: Abstand Bounds-Origin zur View). Gottesstrahlen also nur noch
  innerhalb 8 m, wie im Spiel.
- Nacht-Klasse für Partikel (particles::night_dimmed, NOT from data: dass diese Klassen nachts
  dunkel bzw. hell bleiben – das Spiel hat hier keine Nacht). Daten-Grundlage examples/partmats
  (alle KFX-Pakete, Partikelfarbe = Max Color/ColorOverLife * ColorScaleOverLife):
  * lit (nicht MLM_Unlit, nicht Additive/Modulate), Modulate, und Unlit-Materialien mit eigener
    Beleuchtung (SkyLightContribution / Lichtfarbe, z. B. M_PuffyLitSmoke1): bleiben (folgen dem Licht)
  * Additive: Licht (Mündungsfeuer MF_Flare_AM 0,07-0,8, Funken 50, Feuer M_fire4a_1 1,5-22,
    Emberz) -> bleibt; Ausnahme LDR-Additive aus KFX_Atmosphere (GRay_*, Dust) -> dunkel
  * Alpha-Blend unlit: Farbe > 1 = Feuer (M_firetest_alpha 2-10) -> bleibt; sonst Rauch/Staub/
    Dreck/Trümmer (MF_Smoke <= 0,4, IMP_Soot_Dirt <= 1) -> dunkel; in KFX_Atmosphere immer dunkel
    (Blätter haben Farbe 2)
  Umsetzung: zwei Objekt-Slots (main.rs, vor dem Globals-Buffer) mit objects[].z = 3 (dunkel wie
  Backdrop-Karten: hf_weather_dim, Overcast entsättigt) bzw. 4 (unverändert); platzierte Emitter
  und Pool-Effekte (Einschläge, Explosionen, Mündung) bekommen den MeshTag ihrer Klasse.
- Unlit-Level-Materialien (gamemat.rs daylit_unlit / hf_daylit, NOT from data): MLM_Unlit
  Opaque/Masked/Translucent ohne Himmelslicht/Lichtfarbe/Env-Cube/MaterialTintColor/Szenenfarbe
  werden mit hf_weather_dim gedimmt (Flag 0, nicht 1P – Visier-Leuchtpunkte bleiben). Engelsinsel:
  Graffiti M_graffiti15_c, M_Burnt_Decal_1_dark_C, Pfützen-Translucent-Lagen. Additive (Lampen-
  Glows) und Modulate (Decals, LightPlanes) bleiben.
- Env-Cubes (Aufgabe 3): hf_env_cube dimmt jede Cube-Abtastung einmal; geprüft M_Scope_Lens01:
  Cube-Term wird zum Tint-Term addiert, nicht multipliziert -> kein doppeltes Dimmen; daylit_unlit
  schließt Cube-Materialien aus (sonst dim²). Richtig so. Offen: der Game-Shader-Pfad
  (gameshader.rs, z. B. Overpass-Wasser FFluidVertexFactory) dimmt seine TextureCube-Abtastungen
  NICHT -> Wasser dort nachts evtl. zu hell (nicht geändert, fremde Datei).
- Nebenbefund (nicht geändert): MaterialExpressionClamp – UE3 nimmt bei fehlendem Max nur
  max(x, Min) (cooked PS M_LightPlaneWarehouseBright: max(70*(1-min(z*0,0005,1)), 1)), matcompile
  setzt Max = 1 ein; außerdem ist ScreenPosition im Spiel float4 (Clip-Position, .b = z),
  matcompile liefert vec2 -> LightPlanes sind bei uns immer 1 (unsichtbar). Im Spiel hellen sie
  innerhalb 20 m Innenräume bis 70x auf (Modulate). Korrektur würde Innenräume am Tag stark
  verändern -> erst mit Vergleichsbild.
- Gras vor der Scheune: kein Emitter/Licht in den Daten, das es beleuchtet; vermutlich Mondlicht
  mit der alten Mond/Himmel-Relation (vorheriger Abschnitt: 0,035 / 0,12) – nicht verifiziert.
- Neue Dev-Tools: examples/nearmat (Materialklassen um x,y), examples/partmats (Emitter-Materialien
  mit Blend/Lighting/Partikelfarbe), HB_FXNIGHT=<map-Pakete> --game <Ordner> (Bericht Nachtklasse
  + naga-Validierung, particles::night_report). Engelsinsel: 0 INVALID; GRay/Dust/FlyingTrash/
  SeaBirds/Leaves dunkel, Flies (lit) unverändert.
- Gebaut; im Fenster NICHT getestet. Zu prüfen: Nacht an der Scheune bei Ziel A (kein Kasten mehr,
  von nah sehr schwacher Strahl), Mündungsfeuer/Explosionen nachts hell, Rauch nachts dunkel.

## Lightmaps (statische Mesh-Platzierungen) auf allen Karten, Engelsinsel zuerst (2026-10-21)
- Befund: HomeBots hat bisher auf KEINER Karte Lightmaps geladen (auch nicht auf Kreuzung/
  Overpass); Sonne + Shadow-Maps + Himmelslicht sind dynamisch. Auf der Engelsinsel fällt es
  stärker auf: 567 Platzierungen haben eine 2D-Lightmap (Kreuzung 150, Overpass 148), vor allem
  Innenräume (Baracken 209, Hospital 175, Command 94). Kein fehlendes Paket: die Lightmap-
  Texturen sind Exporte der Sublevels selbst (Outer = Paketobjekt "<Sublevel>_sl", z. B.
  Angel_Geo_Barracks_sl.LightMapTexture2D_38, 1024x512 A8R8G8B8, ein Atlas für alle 209); die
  lightmaps/*_sl.upk liefern nur die gestreamten kleineren Mips (load_full findet sie bei
  Texturqualität < 1024). Terrain / BSP: keine Lightmaps in den Daten (kein LightMapTexture2D in
  Angel_Ter_Terrain, keine ModelComponents).
- Daten (examples/lmscan, lmguids, lmplace, lmalign, lmtex, lmcoord, lmshaders, lmps, neu bzw.
  erweitert): StaticMeshComponent nativ = LODData[] {ShadowMaps[] (immer leer), ShadowVertex-
  Buffers[] (leer), LightMapType (0/2; keine 1D-Vertex-Lightmaps), u32, LightGuids[],
  3 x {Textur, ScaleVector}, CoordinateScale, CoordinateBias}. LightGuids = nur Punkt-/Spot-
  lichter (651/204 Verweise, keine Sonne, kein SkyLight) -> Sonne bleibt dynamisch. Koeffizient
  2 (LQ) überall außer Angel_Geo_Communication/Maintenance (nur 0+1 = HQ). Shader-Cache hat dazu
  FDirectionalLightWithSimpleLightMapTexturePolicy (LQ) und FDirectionalLightWithLightMapPolicy
  (HQ), M_Opaque übersetzt (lmps):
  VS LightMapCoordinate = UV * ScaleBias.xy + ScaleBias.wz (= uv * CoordinateScale + Bias; 785/785
  Lightmaps texelgenau so, vertauscht 1/785);
  PS LQ: Diffuse * max(abs(LightMapTextures.rgb * LightMapScale), 1e-6);
  PS HQ: Diffuse * (LM[0].rgb * Scale[0]) * dot(LM[1].rgb * Scale[1], 1/3); dazu Emissive + Himmel
  + Sonne wie bisher. UV-Kanal = StaticMesh.LightMapCoordinateIndex (nicht serialisiert = 0;
  1100x 1, 67x 2). SRGB: nicht serialisiert -> Default__Texture SRGB true (sRGB-Format).
  Adressmodus: der der Textur (wie alle Texturen).
- Code: upk level.rs LightMap2D + Package::component_lightmaps, Placement.lightmaps (je LOD);
  world.rs static_mesh fügt gamemat::ATTR_LM_UV ein, spawn_mesh nimmt Spawner.lightmap
  (main.rs setzt es pro Platzierung für den gerenderten LOD, 1 Zeile vor spawn_mesh), legt 3
  Zusatz-vec4 hinter den Objektblock (objects[tag+1..3]: Scale/Bias, LightMapScale0+Modus,
  LightMapScale1; gs_blocks parallel aufgefüllt), material_lightmapped = Variante pro
  (Material, Lightmap-Textur); gamemat.rs GameMat lm0/lm1 (Bindings 22-24), GameMatKey.lightmap
  (Vertex-Layout mit Location 6), shader_source_lightmap (Marker-Kommentare HF_LM_* in
  PRELUDE/FRAGMENT), load_texture akzeptiert LightMapTexture2D. Ohne Lightmap unverändert
  (gleicher Shader wie vorher). Unlit-Materialien: keine Variante (wie im Spiel).
- Nacht: keine andere Karte hatte eine Regel (es gab keine Lightmaps). NOT from data: Lightmap
  mal hf_weather_dim() (wie die Tag-Env-Cubes), weil das Bake neben warmen Lampen blaue
  "Tageslicht"-Fülllichter (LightColor 174,190,202) enthält -> nachts so dunkel wie das
  Himmelslicht.
- HB_MAPCHECK druckt je Sublevel "light maps: ..." (Platzierungen, HQ/LQ, Texturen dekodierbar,
  Lightmap-WGSL naga-ok). Engelsinsel: 567 Platzierungen (vorher 0), 2 HQ-Sublevels, alle
  Texturen dekodieren, 196 Materialien WGSL ok, 0 INVALID; Overpass 148 / Kreuzung 150, 0 INVALID.
- Gebaut; im Fenster NICHT getestet. Zu prüfen: Innenräume Baracken/Hospital (warme Lampen,
  vorher schwarz), Communication/Maintenance (HQ), Kreuzung-Tankstelle/Junkyard, Nacht.
  Offen: andere spawn_mesh-Aufrufer (destruction.rs, levelcars.rs, pa.rs) ohne Lightmap (nur der
  Platzierungs-Loop in main.rs setzt sie), Spiel-Shader-Pfad (HOMEBOTS_GAME_SHADERS) ohne Lightmap.

## Schwarze Rauch-"Quadrate" an Fahrzeugen/Wracks (2026-10-21, Nutzer 16:38)
- Ursache: nicht der Rauch selbst. Rauch-Materialien (KFX_VHC Z_Smoke_32FRM_Flat, Tank_Smoke_32FRM_Flat,
  Smoke_Fuel, KFX_Destruction Smoke_thick*/Smoke_IGC) und ihre Texturen (MF_Smoke DXT1 r-Kanal,
  SMK_Fuel/Smk_* DXT5-Alpha, Ränder 0) sind korrekt; gegen den gecachten PS geprüft (Z_Smoke:
  Opazität = Partikel-Alpha * tex.r * DepthBiasedAlpha 1/(1-Bias), identisch). Die Quadrate kommen
  von den Hitzeflimmer-Sprites daneben (Fire_WreckChassis "Distort" M_Fire_Distortion,
  M_Exhaust_Distortion*, Heat_Haze, DSTN_*, auch die Granaten-Rauchspuren M_SmokeTrail_Unlit*):
  Translucent, Emissive = SceneColor an verschobener Bildposition, Opazität 1. Unsere Szenenkopie
  enthält nur die opake Szene -> jedes Sprite malte ein Quadrat opake Szene über Rauch/Feuer dahinter.
- Aus Daten (RefShaderCache M_Fire_Distortion, FParticleSubUVVertexFactory): der Spiel-PS mischt
  selbst, oc0.rgb = lerp(S, E, Opazität) mit S = SceneColorTexture am eigenen Pixel, oc0.a = 0;
  alle SceneColor-Werte auf [0, 4] geklemmt. Das geht nur, wenn S den bereits gezeichneten Rauch
  enthält (UE3 zeichnet SceneColor-Transluzenz nach der übrigen, mit neuem Resolve; Engine-Verhalten,
  im Exe NICHT disassembliert).
- Fix gamemat.rs: scene_blend(c) = Translucent + hf_scene_color; hf_final gibt dann
  Opazität * (E - S_pixel) mit Alpha 0 aus, alpha_mode() -> AlphaMode::Add (world.rs build_material
  nutzt gamemat::alpha_mode). Ergebnis dst + a*(E - S): ohne Transluzenz dahinter exakt lerp(S, E, a)
  wie im Spiel, mit Rauch dahinter bleibt er erhalten (nur die opake Szene wird verzerrt; NOT from
  data). Für diese Materialien kein Nebel/Nacht-Dimmen mehr (die Kopie hat beides schon; NOT from
  data). hf_scene_color klemmt jetzt [0, 4] (aus dem PS).
- Zweiter Befund (particles.rs, upk particles.rs): GCParticleModuleLifetimePerUnit wurde ignoriert.
  Alle Fahrzeug-Schadens-/Wrackrauch-Emitter (FX_WreckSmoke_Medium/Large, Tank_DamageSmoke,
  OA_*_RearSmoke, *_Chassis_WreckFire Cockpit/Rotor_Smoke) haben keine ParticleModuleLifetime ->
  Partikel alterten nie, Alpha blieb beim ersten Schlüssel (0 = unsichtbar) und sie stapelten sich
  das ganze Wrackleben. Neu Module::LifetimePerUnit (Kurve aus Daten, z. B. WreckSmoke_Medium 1,5 s
  bei 0 -> 0,15 bis ~1800, Fire_Damage1 1,2 -> 0,25 bis 1200); NOT from data: Eingang = Emitter-
  Geschwindigkeit in uu/s (aus der Ortsänderung, SystemInstance.speed), Addition zu einer vorher
  gesetzten Lebensdauer (UE3-Konvention). Wracks stehen -> Startwert. Die Wrack-Rauchsäule
  (dunkler Z_Smoke) ist damit erstmals sichtbar.
- Nicht umgesetzt: ParticleModuleSpawnPerUnit (Spawn pro Wegstrecke, bIgnoreSpawnRateWhenMoving) –
  fahrende beschädigte Fahrzeuge spawnen weiter mit der normalen Rate. Nebenbefund: DEC_dynamic_HF
  DM_Parent_Blood01 hat 9 Texturen (> MAX_TEX 8) -> WGSL ungültig (schon vorher).
- Werkzeuge: examples/partalpha (Alpha-Bereiche, Lifetime-/unbehandelte Module je Emitter);
  HB_FXNIGHT prüft jetzt auch jedes Material eines Pakets (Effektpakete ohne Platzierungen),
  HB_FXNIGHT_ERR=1 druckt den naga-Fehler. KFX_VHC/Destruction/Pyrotechnics/Weapons/Impacts/
  Atmosphere/Action: 0 INVALID; Engelsinsel: nur DM_Parent_Blood01 (alt).
- Gebaut; im Fenster NICHT getestet. Zu prüfen: Humvee-Wrack bei Tag aus der Nähe (keine Quadrate im
  Rauch über dem Feuer, Hitzeflimmern sichtbar, Rauchsäule über dem Wrack), Panzer-Schadensrauch,
  Granaten-Rauchspur vor Explosionsrauch, Nacht (Flimmer-Sprites dürfen nicht dunkel sein).

## Fahrzeuge: Schadensstufen vor der Zerstörung (Over-Armor, Schadensrauch) (2026-10-21, Nutzer 16:38 "nur heil oder Wrack")
- Daten: Homefront hat KEINE Morph-Targets / Damage-SkelControls / Gesundheits-Materialstufen für
  die Fahrzeuge, sondern das Over-Armor-System: GCVehicle.ArmorInfo -> GCOverArmorInfo-Archetyp
  (US_M1A3/CH_T99/US_AH64.VehicleSetup.OverArmor, SD_LAV25...OverArmor_US, SD_M1114.OverArmor.
  SD_M1114_OverArmor, SD_MD600_v2...OverArmor; Helis in <Karte>_FrontlinesData), DestructibleBones[]
  {BoneName, Outer, DamagePassThrough, bIgnoreDamage, bBoneIsStatic, VehicleEvents,
  VehicleEventsOnHidden, VehicleMaterialInfos (Dest1/Dest2=1), DestructionSound, ImpactEffect,
  MaterialInfo (HUD-Drahtgitter)}. Knochen-Leben InitialOverArmorHealth 150 (GCVehicle; *_SP 1),
  GCArmorManager bRadialDamageAffectsSingleBoneOnly true. Die Panzerteile sind eigene Knochen mit
  eigener Geometrie (examples/skinfl): M1A3 Schürzen OA_PL1_SKIRT_L0/R0 (außen) + OA_MN1_SKIRT
  (innen), Turmplatten OA_PL1_TURRET_L1/R1 (+L2/R2), Turmheck OA_WS1/WS2_TURRET_REAR_C0, Heck
  OA_WS1/WS2_REAR; LAV Schürzen OA_US_SKIRT_L0/R0, Heck OA_SD_REAR_PRE_DEST -> _DEST, Turm
  OA_SD_TURRET_PRE_DEST -> _DEST, Luke OA_SD_HATCH_R0; Humvee 4 Türen, Motorhaube/Heckklappe
  OA_HOOD_FRONT/REAR_US, WINDSHIELD; AH64 OA_WS1/WS2 TAIL/FRONT/BELLY/ENGINES (+TAILBREAK);
  MD600 OA_WS1/WS2 FRONT/BELLY/TAIL. Effekte über Socket-References (z. B. LeftOuterSkirtFX ->
  KFX_VHC.FX_Tank.OA_M1A3_OuterSkirt_L, RearOuterSmoke -> OA_M1A3_RearSmoke bis VehicleDestroyed,
  RFDoorFX -> FX_Jeep.OA_Jeep_DoorDmg, HoodFX -> OA_Jeep_RoofDmg(B)). Neues Dev-Tool
  crates/upk/examples/overarmor.rs.
- Sichtbarkeit aus der EXE: GCArmorManager::UpdateOverArmorVisibility (0x6666c0) läuft die
  OuterToInner-Ketten ab; versteckt ist ein Teil, wenn sein Bit in RepArmorState gesetzt ist
  (zerstört) ODER sein Outer noch intakt ist (die innere, verbeulte Lage WS2/_DEST/MN1 erscheint
  erst, wenn die äußere weg ist). Versteckt -> HideBone (0x50be60 / 0x472490), zerstört ->
  VehicleMaterialInfos setzen + VehicleEventsOnHidden (0x956270). Max. 32 Teile (Bitmaske).
- Gesundheitsstufen aus Script: GCVehicle.CheckVehicleDamage: Health/HealthMax <
  DamageSmokeThreshhold 0,666 -> VehicleEvent DamageSmoke (Tank_DamageSmoke / LAV_DamageSmoke /
  OA_Jeep_DamageSmoke am DamageFireSocket*), darüber NoDamageSmoke; CheckDamageCriticalAudio:
  unter DamageCriticalThreshold 0,333 mit lokalem Spieler drin DamageCriticalSound (Komponente ohne
  Cue in den Daten -> SCDamageCritical der GCVehicleAudio, falls vorhanden).
- Code: vehload.rs over_armor_raw/damage_cfg (VehicleKind.damage: Teile mit Knochen-Teilbaum und
  Vertex-Grenzen im Fahrzeugraum, Schwellen, Event-References); die *_DEST-Ausblendregel lässt
  Over-Armor-Knochen jetzt aus. vehicles.rs: Vehicle.armor/armor_bits/dmg_events, part_hidden,
  armor_damage (in drive und empty_vehicle_damage vor dem Health-Abzug), damage_states (Rauch-/
  Kritisch-Stufe, Events starten/stoppen Socket-Partikel und -Sounds, folgen dem Socket, beim
  Despawn freigegeben), render_vehicles skaliert versteckte Teile auf 0 (scales_for) und häutet bei
  Zustandswechsel neu. main.rs: 1 Zeile, die Event-Partikel werden gepoolt.
- NOT from data (GCArmorManager.HandleBoneDamage ist nativ, nicht entschlüsselt): Treffer geht an
  das nächste sichtbare, nicht ignorierte Teil (Abstand zu dessen Vertex-Grenzen <= 120 uu, Explosion
  200 uu); Fahrzeug bekommt DamagePassThrough * Schaden + bei Zerstörung den Überschuss *
  (1 - Pass); Trefferort: Explosionen ihr Punkt (Hit.from), sonst der Box-Punkt nächst zum Schützen
  (Hit hat keine Trefferposition; combat.rs unverändert). VehicleEvents und VehicleEventsOnHidden
  feuern beide beim Zerstören.
- Nicht umgesetzt: VehicleMaterialInfos (Dest1/Dest2/dest_*_rear Skalar-Parameter am Fahrzeug-MIC –
  kein Laufzeit-Parameter im Material-Pfad), abfliegende Teile (GCArmorManager.SpawnDebris
  KActorSpawnable – aufrufendes Mesh nicht in den Daten gefunden; die Teile verschwinden, die
  OA_*-Partikel bringen Trümmer), LensFlare/Aftermath-References, HUD-Drahtgitter (UIMaterial),
  Reparatur (NoDamageSmoke nur über HealDamage, das es hier nicht gibt).
- Test: HB_TEST_VEHHITS=<s>,<s>,... (eine volle GCDmgType_ATW_PROX-Rakete je Zeitpunkt aufs
  Spielerfahrzeug, reihum links/rechts/hinten/vorn/oben; mit HB_VEHTEST auf dessen Uhr), HB_DMGDBG=1
  loggt Teil, Teilleben und Schaden. Log-Zeile "vehicle <Klasse> damage: part health ...".
- Gebaut; im Fenster NICHT getestet. Zu prüfen: M1A3 (Schürze weg -> innere Schürze sichtbar,
  Rauch ab 2. Treffer, Heckplatte weg -> verbeultes WS2), Humvee-Türen, ob innere Lagen am Anfang
  wirklich unsichtbar sein sollen (vorher wurden M1A3-WS2/MN1 immer gezeichnet).

## Nacht: untere Hälfte der Häuser hell, heller Keil am Boden neben dem LAV (2026-10-21, Nutzer 18:27)
- Ursache (Daten + unser Shader): der untere Himmelslicht-Term nahe am Gelände ist
  GrassColorTexture * LowerSkyColor.w (UE3-SkyLight, gamemat.rs Lit-Pfad, Spiel-Shader
  LowerSkyColor.w, terrainemu). w = SkyLightComponent.LowerTerrainBrightness, Engelsinsel 6,0
  (SkyLight_1, Falloff 80-100 uu). weather.rs dimmte nachts nur lower_sky.rgb, w blieb der
  Tageswert. Folge: alles innerhalb ~1 m über der Geländehöhe unter dem Objekt-Ursprung (untere
  Wandteile von Häusern/Containern, alles darunter) behielt den vollen Tages-Bodenreflex
  (Graswert * 6 = 10-25x lower_sky.rgb), der Rest lag bei 12 %. Gleiches auf dem Gelände selbst:
  Hänge (Normale nicht senkrecht) bekamen den ungedimmten Term -> heller, bläulich wirkender Keil
  am Hang unter dem Felsen rechts vom LAV (rec_st1 f00003-f00012). Lightmaps waren es nicht
  (schon mit hf_weather_dim), Nebel auch nicht (Nebelfarbe folgt dem gedimmten Himmelslicht).
- Fix weather.rs: lower_sky.w wird mit dem Luminanz-Verhältnis des unteren Himmelslichts
  (jetzt / Level) skaliert (NOT from data; Nacht, Bewölkung, Schnee folgen). Wetter aus =
  Level-Wert wie vorher. Wirkt auf gamemat, terrainemu und Spiel-Shader (alle lesen Globals).
- Zweiter Befund gameshader.rs: UpperSkyColor/LowerSkyColor (Pro-Objekt-Konstanten der
  SkyLight-Policy) kamen aus dem Objektblock (Ladewert, weather.rs schreibt ihn nie) ->
  SpeedTrees/Wasser (und alles mit HOMEBOTS_GAME_SHADERS) hatten nachts das volle Tages-
  Himmelslicht (heller Baum im Sturmtest). Jetzt hfg.upper_sky / hfg.lower_sky (gleiche Werte
  bei Tag, gedimmt bei Nacht). Straßen nutzen MaterialUpper/LowerSkyColor + PrimaryLightColor
  (matps: auch der MLM_Unlit-Rand liest c10-c12) -> waren schon gedimmt.
- gameshader.rs (NOT from data): Nachtklasse wie gamemat hf_daylit für Spiel-Pixelshader ohne
  Licht-/Himmels-/Ambient-Konstante, Cube-Map und Szenenfarbe in der Konstantentabelle (Opaque/
  Masked/Translucent): oc0.rgb * hf_weather_dim(). Betrifft auf der Engelsinsel keine Straße
  (alle haben die Konstanten), nur echte Unlit-PS.
- Geprüft: gebaut; HB_MAPCHECK=AngelIsland: alle 4 Straßenmaterialien Spiel-Shader-WGSL ok,
  Lightmap-WGSL 0 INVALID. Im Fenster NICHT getestet. Zu prüfen: Nacht an Häusern/Containern
  (kein heller Sockel mehr), Hang neben dem LAV im Sturm, Bäume nachts, Tag unverändert.
- Offen: der Bodenreflex verliert nachts die Blautönung (Skalar statt Farbe); Spiel-Shader-
  Cube-Abtastungen (Wasser) weiter ungedimmt; dünne Linie links unter dem LAV (f00006) nicht
  untersucht (vermutlich Leitung/Draht).

## Vorladen vor dem ersten Spawn (2026-10-21, Nutzer 18:27 "Ich will, dass alles vorlädt")
- Befund (test_d1/nb1, Engelsinsel): nach "Movie -> Playing" erschienen Felsen/Häuser erst nach
  ~3 s (rec_nb1 f00002 vs f00016), Himmel im ersten Bild schwarz. Ursache: Bevy baut die Render-
  Pipeline eines Meshes erst beim ersten Zeichnen und kompiliert sie asynchron; bis dahin fehlt
  das Mesh. Die Szenenkamera war während Ladebild/Film aus, das Vorwärmen (warmup.rs) lief nur in
  der Spawnauswahl – die Tests (HOMEBOTS_AUTOSPAWN) überspringen sie in 0 Frames, im Spiel ist sie
  oft kürzer als die Kompilierzeit. Texturen/Meshes selbst werden NICHT nachgestreamt: load_maps
  baut alles synchron (Mips bis zur Texturqualität), nur der GPU-Upload passiert im nächsten Frame.
- Neu (warmup.rs, menu.rs): sobald das Match-Level gebaut ist, startet ein Preload. Die Szenen-
  kamera rendert hinter dem schwarzen Ladebild / dem Film (CameraOutputMode::Skip, das Bild geht
  nicht ins Fenster), die Stellvertreter (je Material + Vertex-Layout, 50 km hinter der Kamera)
  entstehen jetzt schon dort; zusätzlich GrassMat und StandardMaterial (Nuke-Modelle), auch
  Nicht-Level-Entities. Ein Render-Welt-Probe zählt: Pipelines in Queued/Creating
  (PipelineCache, nach process_queue; fehlerhafte mit Retry zählen nicht), Images ohne GpuImage,
  Materialien (GameMat/TerrainEmu/HfMat/GrassMat) ohne PreparedMaterial. Fertig, wenn 10 Frames
  in Folge nichts davon offen ist (frühestens 4 Render-Frames nach den Stellvertretern).
  Abbruch: nichts kompiliert und 2 s kein Upload-Fortschritt (Material ohne Textur), 8 s gar kein
  Fortschritt, oder 120 s gesamt (Warnung im Log).
- Ablauf: Film läuft weiter wie bisher; Enter/Filmende/Auto-Skip vor Preload-Ende -> Ladebild
  mit Spinner (Phase Loading, await_preload) bis fertig, dann Spawnauswahl. Ohne Film und beim
  Bauen nach dem Film ebenso. Stellvertreter bleiben bis Playing. HB_NOWARMUP=1 schaltet alles ab.
- Log: "preload done: N pipelines (M compiled during the preload, K ready), T textures,
  U materials on the GPU, S warm-up stand-ins, X s after the level build" und
  "preload: loading screen held X s after the movie" (Wartezeit, die der Spieler sieht).
- Prefetch konsistent (main.rs preopen, upk library.rs): bisher wurden alle Mips <= Kappe aller
  Texture2D der Levelpakete gelesen, der Build brauchte aber andere (Test d1: 3232 Lesungen, nur
  1299 aus dem Prefetch, 1529 Prefetches ungenutzt). Jetzt zeichnet der Build jede Separate-
  File-Lesung auf und speichert die Liste (%TEMP%\homebots_prefetch\<hash>.txt, Hash aus Karten,
  Texturkappe, BC-Pfad); der nächste Preopen derselben Karte liest genau diese Liste (auch aus
  Waffen-/Fahrzeug-/Effektpaketen). Erster Lauf ohne Liste: Heuristik, jetzt nur die Mips, die
  der Build nimmt (DXT mit BC: Kette ab Kappe, sonst nur die oberste Mip). Log: "preload: build
  read N texture mips, H from the prefetch, D from disk during the build, U prefetched but not used".
- NICHT aus Daten (Renderer-Detail). Gebaut; im Fenster NICHT getestet. Erwartung: kein Pop-in
  nach dem Spawn; Ladebild beim sofortigen Überspringen des Films ~1–4 s länger (Kompilierzeit auf
  dem PC), beim Anschauen des Films 0 s; ab dem 2. Laden derselben Karte ~0,8 s schnellerer Build.
- Zu prüfen: Logzeilen oben; ob während des Films kein Spielbild durchscheint; Spawn ohne
  Nachladen (Felsen, Häuser, Himmel im 1. Bild); Pipelines, die erst im Spiel entstehen
  (Minimap-Kamera, neue Kamera-Komponenten, Effekte mit eigenem Layout zur Laufzeit) sind nicht
  abgedeckt.

## Testszenarien d1/d2/d3, nb1/nb2, sr1, vd1 neu platziert (2026-10-21, Nutzer 18:27)
- Problem: d1/nb1 zeigten ihre Ziele nicht, vd1-Kamera fuhr ins Haus. Positionen jetzt aus den
  Kartendaten: neues Dev-Tool crates/upk/examples/viewspot.rs (Terrain + Static-Mesh-Dreiecke im
  Umkreis 45 m; pro Richtung/Abstand: begehbare Fläche mit 2 m Kopffreiheit nahe der Ziel-Basis,
  Zylinder frei, Sichtlinie Auge -> 5 Punkte der Ziel-Bounds; Ausgabe HOMEBOTS_SPAWN_AT, Pitch,
  ROOF = Decke darüber). VS_OPEN=<r>: freie, flache Flächen für Fahrzeugtests. lmnear.rs: größte
  Platzierungen um einen Punkt mit Lightmap-Flag.
- HOMEBOTS_SPAWN_AT=x,y,z,yaw,pitch: optionaler 5. Wert = Blick-Pitch in Grad (player.rs).
- HB_VEHTEST_STILL=1: Testfahrzeug fährt nicht (Turm/Waffen-Skript läuft), vehicles.rs.
  HB_VEHCAM: Kamera wird vor die erste Wand/Terrain zwischen Fahrzeug und Auge gezogen.
- Szenarien: d1 Torpfosten indGateHalf (Barracks) quer aus 6 m, Mittag; d2 Barrack_Building
  (Lightmap) NW-Ecke aus 6 m; d3 Bauernhaus innen (Farm1_Wall_Int_Door_Solid, 5 m, Decke 2,1 m);
  nb1 Scheune Ziel A (Farm1_Extension_C + A_End-Tor, Emitter_52) von außen aus 9 m, Nacht;
  nb2 Hauswand Farm1_Combined_A frontal aus 5 m, Nacht; sr1 Bildschirmregen tags (d2-Blick);
  vd1 M1A3 steht auf freier Fläche (398,-8396: kein Mesh im 10-m-Kreis, Terrain +-1,2 m,
  nächster SpeedTree 7,5 m außerhalb des Bildes), HB_VEHCAM=40,650,250. Sichtlinien offline
  geprüft (Dreiecke, SpeedTrees nur bei vd1); im Spiel NICHT getestet.

## Pipeline-Kompilierung im Preload: Messung, Uhr, Threads, Shader-Defs (2026-10-21, AMD-iGPU)
- Befund Uhr: Die "~95 s" (test_d2/nb1/sm1/vd1 "preload done ... 96.40 s after the level build")
  waren keine Sekunden. Die Tests laufen mit HB_RECORD (5 fps): TimeUpdateStrategy::ManualDuration
  schiebt auch Time<Real> um 0,2 s pro Bild vor. 96,4 s = 482 Bilder. Laut Log-Zeitstempeln (Wanduhr)
  d2: Level gebaut 17:30:03,4, Preload fertig 17:30:11,0 = 7,6 s für 485 Pipelines (~16 ms je
  Pipeline bei ~64 fps); "loading screen held 70.80 s" waren 5,1 s. d1 (erster Lauf nach dem Build)
  war 2,7x langsamer (in 9,1 s nur ~217 fertig) -> der AMD-Treiber cacht ab dem 2. Start bereits
  selbst. Folge des Zwischenstands: die 25-s-Kappe (HB_PRELOAD_MAX) hätte im Test nach 125 Bildern
  = ~2 s Wanduhr abgebrochen.
- Fix warmup.rs/menu.rs: Preload-Timer, Kappe (25 s, HB_PRELOAD_MAX), QUIET/STALL und die
  Logzeilen laufen über warmup::wall() (std::time::Instant). Logzeile jetzt
  "preload done: ... X s (wall clock, N render frames) after the level build, T compile threads",
  "preload: loading screen held X s (wall clock) after the movie".
- Messung offline (Container, Xeon 2,1 GHz, lavapipe/Vulkan; Bench scratchpad/plbench mit
  naga 24 / naga_oil 0.17.1 / wgpu 24.0.5 wie Bevy 0.16, 251 unserer generierten WGSL, 12 KB
  Quelle, 22 KB zusammengesetzt), je Shader/Pipeline:
  naga_oil compose (Bevy ShaderCache, SERIELL unter dessen Mutex) 4,7 ms; naga Validierung 0,15 ms;
  naga SPIR-V (beide Stufen, pro Pipeline in wgpu-hal) 0,35 ms; lavapipe vkCreateGraphicsPipelines
  ~5,5 ms. Ohne Imports (vorab zusammengesetzt) kostet naga_oil sogar 5,7 ms (reiner naga-Parse
  1,4 ms): die Kosten sind naga_oils eigene Vorverarbeitung, nicht die Bevy-Imports.
  wgpu-Pipelines skalieren mit Threads (1 Thread 6,45 ms, 2 Threads 3,66 ms je Pipeline).
- Bevy 0.16 (bevy_render pipeline_cache.rs): jede Pipeline ist ein Task im AsyncComputeTaskPool
  (multi_threaded ist an); Shader-Module werden je (Shader, Shader-Defs) gecacht; Composer +
  create_shader_module laufen unter einem Mutex, der Treiber-Aufruf parallel. Pool-Standard:
  25 % der logischen Kerne = 2 Threads auf dem Ryzen Z2 A (8 Threads).
- Fix main.rs task_pools(): AsyncCompute bekommt alle logischen Kerne minus 2 (mind. 2;
  HB_COMPILE_THREADS=<n>), IO/Compute behalten Bevys Standardgrößen. NICHT aus Daten.
- Fix gamemat.rs canonical_defs() (GameMat + TerrainEmu specialize): die Shader-Defs der Pipeline
  (MAY_DISCARD, VERTEX_COLORS, DEPTH_PREPASS, DEPTH_CLAMP_ORTHO der Sonnenschatten, Tonemapping, ...)
  werden auf die wenigen reduziert, die unsere WGSL sehen kann (PER_OBJECT_BUFFER_BATCH_SIZE,
  SIXTEEN_BYTE_ALIGNMENT, MESHLET_MESH_MATERIAL_PASS, NO_*_TEXTURES_SUPPORT, WEBGL2). Unsere WGSL hat
  keine eigenen #ifdef (HF_MASKED/HF_UNLIT löst gamemat.rs beim Erzeugen auf) und importiert nur
  view/globals/get_world_from_local/get_tag. Damit wird jeder Shader einmal zusammengesetzt statt
  einmal pro Pass/Mesh-Layout. HB_KEEP_SHADER_DEFS=1 = altes Verhalten. NICHT aus Daten.
- Dedupe gleicher WGSL gab es schon (world.rs shader_cache nach Quelltext, auch _depth).
- Nicht umgesetzt: persistenter VkPipelineCache – Bevy 0.16 setzt cache: None fest (pipeline_cache.rs
  start_create_render_pipeline), dafür bräuchte es eine gepatchte Kopie von bevy_render (1,5 MB)
  über [patch.crates-io]; lavapipe liefert nur einen 96-Byte-Cache, also hier nicht messbar; der
  AMD-Treiber cacht laut d1/d2 schon selbst. SPIR-V-Plattencache (Bevy-Feature shader_format_spirv,
  eigenes naga_oil-Zusammensetzen beim Level-Build, Datei je WGSL-Hash) würde die serielle
  Composer-Zeit (~4,7 ms je Modul) ab dem 2. Start fast ganz sparen – nächster Schritt, falls der
  Preload noch > 3 s braucht. Priorisierung nach Spawn-Kamera: Bevys PipelineCache hat keine
  Reihenfolge (HashSet); bei wenigen Sekunden Gesamtzeit nicht nötig.
- Erwartung AMD-iGPU (2. Start, Treiber-Cache warm): vorher 7,6 s Wanduhr; seriell ~485 x ~5 ms
  Composer = ~2,4 s Untergrenze, jetzt weniger Module (Defs) und Treiberarbeit auf 6 statt 2
  Threads -> grob 2–4 s. Gebaut; im Fenster NICHT getestet.
- Zu prüfen (tests\run_test.cmd, logs\test_*.err): "preload done" mit "wall clock" und
  "6 compile threads"; Wert für d2/nb1 im 2. Lauf; Bild beim Spawn vollständig (keine fehlenden
  Meshes, Schatten korrekt; bei Fehlern HB_KEEP_SHADER_DEFS=1 gegenprüfen); Film ruckelt während
  des Preloads nicht (sonst HB_COMPILE_THREADS=4).

## Ton: Knacken / Ticken im Bot-Gefecht (2026-10-21)
- Rückmeldung 20:10: "Knacken und Ticken im Ton, wenn Bots gegeneinander kämpfen".
- Nachgestellt ohne Fenster/Gerät: examples/sfx_burst.rs (cargo run --release -p homebots --example
  sfx_burst -- <HOMEFRONT> <ausgabe> [bots]). 16 Bots schießen 8 s in Feuerstößen (echte 3P-Fire-Cues
  aus A2_Weapons_C mit MaxConcurrentPlayCount 6, Cue-Lautstärke 1,5, Attenuation 400..30000 aus den
  Daten, Gruppe Weapons3P 1,26), dazu Einschläge, Schritte, 2 Schleifen mit Lautstärke je Frame.
  Alt = rodio-Sink je Sound wie bevy_audio + bisheriges Kanallimit/Ausblenden, Neu = mixer.rs.
- Ursachen (gemessen, 16 Bots / 4 Bots):
  1. Übersteuerung: rodio summiert alle Sinks ohne Begrenzer, das Gerät klemmt bei 1,0. Ein naher
     3P-Schuss allein spielt mit 1,5 x 0,8 = 1,2. Alt: 63,8 % / 35,9 % der Samples geklippt,
     Spitze 12,0 / 6,0. sfx.rs teilt die Master-Kette DistortionMaster .3 x FL2SubMaster .4
     (= -18,4 dB Reserve im Spiel) heraus, daher fehlt jede Reserve.
  2. Klicks beim Stoppen / Lautstärkeänderung: rodio setzt die Lautstärke alle 5 ms als Sprung;
     das 3-Frame-Ausblenden (x0,35 je Frame) waren drei Sprünge plus Schnitt, jede Abstandsänderung
     einer Feuerschleife ein Sprung. Bei 548 MaxConcurrentPlayCount-Neustarts in 8 s (68/s).
     Sinus-Test (max. |2. Ableitung|, sauber 0,005): alt 0,161, neu 0,006.
  3. Last im Audio-Thread: je Sink ~10 Iterator-Adapter + Mutex-Abfrage je 5 ms. Alt 170–198 ms
     CPU je Sekunde Ton (16 Bots, Container-CPU), neu 40–46 ms; auf dem Z2 A entsprechend mehr ->
     Gefahr von Pufferunterläufen (= Knacken) bei voller Spiellast.
- Fix: crate::mixer (neu, mixer.rs) mischt alle Spielsounds selbst; bevy_audio spielt nur noch EINE
  Quelle (MixerBus, Stereo f32 in der Geräte-Abtastrate über cpal default_output_config, also ohne
  rodio-Resampling). Wellen werden einmal zu f32-PCM dekodiert (Ende auf die Granule-Position
  gekürzt wie bisher), lineare Interpolation für Tonhöhe/Abtastrate, jede Lautstärke-/Tempoänderung
  pro Sample geglättet (4 ms), Start-Rampe 0,7 ms, Stopp/Verdrängung linear über 20 ms. Kanallimit 64
  ([XAudio2.XAudio2Device] MaxChannels, aus Daten) jetzt im Mixer (Auswahl wie bisher: One-Shots vor
  Schleifen, leiseste, älteste – NOT from data). Summenbus x0,5 (-6 dB), dann Look-ahead-Begrenzer
  (1,5 ms, Schwelle 0,85, Release 0,3 s) und Soft-Clip ab 0,9 -> nie über 1,0. Alle Zeitkonstanten,
  Schwelle und Bus-Pegel NOT from data. Neu gemessen: 0 geklippte Samples, Spitze 0,68 (nach
  GlobalVolume 0,8); Begrenzer im 16-Bot-Gefecht median 13,4 dB (p95 16,0), bei 4 Bots 7,6 dB.
- Nebeneffekt: keine Audio-Entity mehr je Sound (play_queued_audio_system 2,4 ms / p95 11,7 ms im
  Profil vom 18.10. entfällt für Effekte). Kleine Wellen (< 48 KB Ogg) werden sofort dekodiert, große
  im Hintergrund – die Stimme wartet im Mixer stumm (max. 2 s) statt die Ogg-Fassung zu spielen.
  Sounds sind insgesamt ~6 dB leiser als vorher (Bus 0,5); Einzelsounds klingen sonst gleich.
  Video-Ton (movie.rs) und Lautsprecherdurchsagen (pa.rs) laufen weiter direkt über bevy_audio.
- Vorher/Nachher-WAVs (Spielsounds, nur lokal): /mnt/user-data/outputs/audio-2025/
  (before_rodio.wav / after_mixer.wav, Unterordner 4bots). Gebaut; im Fenster NICHT getestet.
- Zu prüfen am PC: Gefecht mit vielen Bots anhören (kein Knacken/Ticken, Pegel ok?); HB_PERF=1:
  Zähler audio_voices / audio_voices_culled; Log "sfx: mixer bus at <rate> Hz". Zu leise -> BUS_GAIN
  in mixer.rs (0,5) anheben; "pumpt" hörbar -> LIM_RELEASE_S.

## Zerstörbare Map-Autos (2026-10-21, Nutzer 20:10 "Türen fallen ab, Scheiben zerspringen, Motoren explodieren")
- Daten (MP-Karten Crossroads / Overpass / Angel Island): alle platzierten Autos sind
  GCInteractiveSkelMeshActor aus VEH_Jiggle.cars.Prefabs.DSO_*_Blocking (Auto + GCVehicleBlockingVolume),
  GCInteractiveObjectInfo II_CarCivilian01/02, II_TruckCivilian03, II_UtilityVan, II_Semi_Cab,
  II_Bulldozer_01, DEST_HumV_Destroyed / DEST_Buffalo_Destroyed (FL-Crossroads 90, FL-Overpass 90,
  FL-AngelIsland 17, Crossroads-Unterlevel 1-25). GMS_ExplodingCars (Busse/LKW/Wohnmobile mit
  InterpActor/KActor/Kismet/GCAftermathTrigger) wird von KEINER MP-Karte referenziert (nur SP).
- Was die Daten/EXE festlegen (neues Tool crates/upk/examples/sockrefs.rs: alle Socket-References inkl.
  der objektlosen; callers.rs; EXE per objdump):
  - Schwellen pro Socket (EXE 0xd64cf0, GCInteractiveObjectInfo-Init): StartTime der References mit
    StartTag "Destroyed" = DestroyThreshold, "Cracked" = CrackThreshold (Fenster 110 / 150,
    Scheinwerfer/Rücklichter - / 50). Name 0x12d = Destroyed, 0xcf5 = Cracked, 0xcf6 = Explosion.
  - HandleBoneDamage (0xb1d750): Schaden sammelt pro Socket (DamageReceived, max 16); über Destroy ->
    "Destroyed"-References (WindowExplode_Side + SC_Debris_Glass, Head-/TaillightExplode +
    BulletGlass06) und HideBone; sonst über Crack -> "Cracked": objektlose Reference = Material-
    Skalarparameter EndTag := EndTime (Window1..6Shatter = 1 auf M_common_Glass_LF, Sprung-Overlay).
  - Kugel (0x988290): HandleBoneDamage(Schaden, getroffener Knochen), sonst PlayImpactEffectsEx.
  - Explosion (0x6d7440): jeder Knochen innerhalb DamageRadius bekommt int(BaseDamage) ohne Abfall,
    danach PlayExplosionEffects (0x9918a0): Root-Sockets "Explosion" (Car_GroundDust +
    Destrustion_Metal04), OnExplodeAnim "Explode" (Ruck des Root), AddRadialImpulse.
  - Fahrzeugkontakt (0x411060): schneller als ExplodeSpeed 120 -> alle Sockets zerstört + Explosions-
    effekte, schneller als BlowOutSpeed 60 -> alle Sockets zerstört.
  - NICHT in den Daten: Brand, Motorexplosion, abfallende Türen, ausgebranntes Material. Türen/Haube/
    Kofferraum hängen an RB_ConstraintSetup zu Root, nicht brechbar, Schwenk/Drehung auf 5 Grad
    begrenzt (nur "Wackeln"). Das Original-MP hat genau Glas/Lichter + Ruck + Staub.
- Ausrichtung: Ursache des "hochkant"-Autos = Roll-Vorzeichen in levelcars rot_of (Z*Y(-p)*X(+r));
  jetzt FRotationMatrix wie main.rs rotator_to_quat (Unit-Test geschrieben, nicht ausgeführt).
  Offline geprüft mit neuem Tool crates/upk/examples/carorient.rs (Welt-Grenzen jedes platzierten
  Autos, z-Ausdehnung = Höhe): FL-Crossroads 90/90, FL-Overpass 90/90, FL-AngelIsland 17/17 aufrecht,
  Unterlevel alle bis auf ein absichtlich gekipptes HumV_Destroyed (Roll -70 Grad). Gate
  HB_SKINCARS entfernt; HB_STATICCARS=1 = alte Referenz-Meshes ohne Schaden.
- Umgesetzt (levelcars.rs): Schwellen, Sammeln, Destroyed/Cracked/Explosion-Effekte am Socket mit
  Socket-Drehung, Knochen ausblenden, Explosionsschaden pro Knochen im Radius (combat.rs ruft
  levelcars::blast für Raketen und taktische Explosionen), OnExplodeAnim, OnShotAnim; Kugeln über
  impacts.rs world_hit (jetzt mit Schadenstyp). Kollision für Spieler/Bots/Schüsse unverändert
  (Render-Dreiecke der Referenzpose).
- NOT from data: getroffener Knochen einer Kugel = nächster Knochen der aktuellen Pose (PhysX-Körper
  nicht dekodiert); Schaden = Damage-Grid-Wert für PM_Default (PhysicalMaterial der Autokörper nicht
  dekodiert; z. B. ATW_PROX 200 -> alles im Radius zerbricht, M4-Kugeln mehrere Treffer); von einer
  Explosion erreicht = Hüllkugel im DamageRadius, ohne Sichtlinie; OnShotAnim als PlayImpactEffectsEx.
- Nicht umgesetzt: Material-Schalter "Cracked" (keine Laufzeit-Skalarparameter), Radialimpuls
  (Türen wackeln 5 Grad), SoundNodeWave-References (Queue spielt nur Cues 3D), Bildgrößen-Gate
  MinEffectsDisplayFactor 0,3, Fahrzeugkontakt, DrawScale der Akteure (alle 1 in den Daten).
- Test: HB_TEST_CARHITS=<s>,<s>,... (ATW_PROX-Explosion am nächsten Auto, auf HB_RECORD_PLAY-Uhr;
  record.rs play_clock), HB_CARDBG=1 loggt Schwellen, Welt-Grenzen, Socket-Schaden. Szenario ch1.
- Gebaut; im Fenster NICHT getestet. Zu prüfen am PC: ch1 (Auto steht richtig, Scheiben/Lichter
  zerspringen beim 1. Treffer, Glas weg, Staub + Ruck), Autos auf allen Karten aufrecht.

## Map-Autos explodieren wie in der Kampagne (2026-10-21, Nutzerentscheidung: bewusst NICHT original fürs MP)
- Quelle: SP-Paket Vehicles_FL2/GMS_ExplodingCars (Prefabs trucks.TruckCab1 / RV1-3 / NKTruck /
  NKGasTruck / buses.CityBus1; neues Tool crates/upk/examples/kismetdump.rs = Kismet-Graph mit
  Links/Variablen, pathfull.rs = alle Properties unter einem Pfad). Übernommen (im Code "SP"):
  - Leben 1000: SeqEvent_TakeDamage_1 (GCDamageType, DamageThreshold 1000) in jedem Prefab -> Explosion.
  - "Burndown" 5 s: TruckCab1 SeqEvent_TakeDamage_138 -> SeqAct_Delay_27 -> Explosion.
  - Explosion: KFX_Pyrotechnics.FX_Explosions.Vehicle_Civilian2 + A2_Destruction_C.
    SC_LightVehicleAnnihilation_SP (TruckCab1), SeqAct_CauseDamageRadial 1000 / Radius 1024 /
    bDamageFalloff / GCDmgType_Veh_HeliMissile -> combat.rs (Soldaten, Spieler, Drohne, Fahrzeuge,
    andere Autos = Kettenreaktion; Auslöser Actor::World, trifft jedes Team; Grid-Wert PMA_Soldier
    ersetzt 1000, wenn vorhanden, wie destruction.rs).
  - Brennendes Wrack: KFX_VHC.FX_LAV.Fire_WreckChassis (TruckCab1 GCActorFactoryGCEmitter_2; bei
    RV/NK-LKW ist "Clean up Fire" nicht verknüpft -> brennt weiter).
  - Wrack-Abwurf InitialVelocity z 750 (ActorFactoryPhysicsAsset): hier fliegen die noch
    hängenden Türen/Haube/Kofferraum damit hoch (Karosserie bleibt stehen = Kollision bleibt).
  - Abfallende Teile: Gelenke des PhysicsAsset (II PhysicsAsset, PA_DEST_*: ConstraintBone1 ->
    Root) mit Kind-Knochen (Fenster). CivilianCar01: 4 Türen + Haube + Kofferraum; CivilianCar02 /
    TruckCivilian03 / Semi / Truckcab / HumV / Buffalo: Türen; Minivan, Utilityvan, Bulldozer,
    Müllwagen: keine (nur Feuer/Explosion/verbrannt).
- Ablauf (levelcars.rs): Stufe 1 Glas/Lichter wie bisher; Stufe 2 ein Teil fällt ab, sobald seine
  Knochen 300 Schaden haben (Flug mit destruction.rs PIECE_*/GRAVITY/BOUNCE, Boden über
  CollisionWorld.floor); Stufe 3 Motorbrand ab 500 (halbes Leben) an Haube/Scheinwerfern,
  Explosion bei 1000 oder 5 s nach Brandbeginn; danach alle Scheiben weg, Staub + Ruck, Teile
  fliegen hoch, Wrackfeuer, Auto verbrannt.
- NOT from data: Schwellen 300 / 500, Motorbrand-Effekt KFX_Pyrotechnics.FX_Fire.Fire_Car (in den
  Daten, aber von keinem SP-Kismet benutzt; die SP-Prefabs brennen vor der Explosion nicht) und
  seine Position, Brand-Loop A2_Destruction_C.SC_FireLoop_Metal (MP-Wrack-Socket-Reference),
  Teile-Physik, Wrackfeuer-Position, Explosionslicht GCExplosionLight_Rocket, Teile-Schaden aus
  Schüssen (nächster Knochen). "Verbrannt" ist NUR eine Abdunklung: VEH_Jiggle hat kein
  verbranntes Material (SP tauscht per SeqAct_SetMaterial *_Wreck/*_DEST-MICs anderer Meshes) ->
  gamemat.rs objects[].z = 5 (Diffus entsättigt * 0,12, Glanz * 0,08), von levelcars.rs per
  BurntUpload in den GameMat-Globals-Puffer geschrieben (Pfad mit HOMEBOTS_GAME_SHADERS nicht
  abgedeckt). Nicht umgesetzt: GCAftermathTrigger AM_VehicleExplosion, LensFlare
  LF_Radial_Heli_Crash, RB_RadialImpulse 10000/300, Momentum 1e6 auf Opfer.
- main.rs: KFX_VHC in die Paketliste (Wrackfeuer). Kollision unverändert (Referenzpose).
- Szenario ch1 neu: Spieler 13,5 m entfernt (viewspot dir 60, außerhalb des 1024-uu-Radius, auch von
  Nachbarauto _128 bei 10,9 m), neues HB_TEST_CAR=<x>,<y> wählt das Auto nahe dem Punkt (_97),
  HB_TEST_CARHITS=2,4,6,8, Aufnahme 1-14 s: 2 s Glas, 4 s Türen/Haube/Kofferraum fliegen, 6 s
  Motorbrand, 11 s Explosion + Wrackfeuer + dunkel.
- Gebaut; WGSL per HB_MAPCHECK=Crossroads:FL (naga) gültig; im Fenster NICHT getestet. Zu prüfen
  am PC: ch1 (Teile fliegen und liegen am Boden, nicht im Auto; Feuer an der Motorhaube; Explosion
  tötet den Spieler nicht; Wrack dunkel; Nachbarautos ggf. Kettenreaktion).

## Leuchtspur-Licht gedimmt (2026-10-21 abends)
- Wunsch des Users ("sieht aus wie Star Wars"): Leuchtspur-Licht (dynlights.rs) und das Leuchten des Tracer-Sprites (TracerColor in combat::fire_tracer) nachts um 20 %, tagsüber um 40 % schwächer; dazwischen gleitend über den Tageslicht-Faktor `weather::daylight()` (neu, day_k aus `light()`; 1 bei abgeschalteter Wetter-Simulation). NOT from data – Nutzerwunsch. Raketen-, C4- und Werfer-Lichter unverändert.

## PA-Mast neben die Straße (2026-10-21, 22 Uhr)
- User (Screenshot Crossroads, B 27 m): der Lautsprechermast stand mitten auf der Straße auf der gelben
  Mittellinie. Der Mast ist NICHT aus den Kartendaten, sondern unser eigener (pa.rs). Ursache: die freie
  Stelle wurde 6–16 m um Ziel B gesucht (eben, nichts darüber) – die Ziele B der Linien 0 und 2 liegen
  auf der Straße (Straßenspline-Mitte 1 m bzw. 4 m entfernt, Fahrbahn ~9 m halbe Breite, Daten:
  Crossroads_terrain RoadSplineMesh), und die ebene, freie Fahrbahn war die erste passende Stelle.
- Neu: CollisionWorld merkt sich Straßenflächen (mark_road / is_road, Dreiecksbereiche): die
  Straßenspline-Meshes und die Straßenset-Statikmeshes (STA_highway.roadSet.*Lane*, auch die
  Überführungsdecks; *.Road.Road_*), main.rs is_road_mesh. pa.rs spot_ok: kein Straßendreieck unter
  dem Mast und unter 8 Punkten im Umkreis von 1,7 m (1,5 m Abstand zum Fahrbahnrand + Mastradius);
  Suche 6–25 m. Der feste Mast an der Überführung (Linie 1) wird ebenfalls geprüft (erst der Punkt,
  dann Ringe ab 2 m). Gilt für jede Karte mit Masten (derzeit nur Kreuzung). Kollision des Mastes wie
  bisher am selben Ort wie das Bild. NOT from data – Wunsch des Users.
- Gebaut; im Fenster NICHT getestet. Szenario pa1 (Blick aus 24 m entlang der Straße auf B der Linie 0):
  Mast neben der Fahrbahn, Log "pa: pole at ..." (sonst Warnung "no free spot off the road").

## Fadenkreuz verschwindet über dem Straßenrand vor der Tankstelle (2026-10-21, Nutzer 21:59, Kreuzung, Regen)
- Der Streifen: Crossroads_terrain RoadSpline_5 (Akteur bei UE -12798,-221, läuft über Ziel B /
  Tankstelle bis -5522,1377), Mesh Roads.Statics.roadSection1, Abschnitt 1 mit
  M_asphalt_road_edge_crossroads (MIC, Parent Roads.Materials._Parent.M_Road_Translucent =
  Translucent) = der Randstreifen beidseits der Fahrbahn (neues Tool examples/roadsecx: Abschnitte
  eines Straßen-Meshes mit lokaler Breite).
- Ursache (Sortierung, nicht Shader/Nebel/Szenenfarbe): bevy sortiert Transparent3d nach der
  Sichttiefe der Transform-Translation (bevy_pbr 0.16 queue_material_meshes,
  distance_translation). Die 1P-Meshes (Waffe, Visier-Glas, Reticle M_Crosshairs_HoloParent)
  stehen am Auge (Tiefe ~0). Straßen-Splines haben ihre Translation am Spline-Anfang und werden
  bis ~100 m weit verformt: steht der Spieler so, dass der Anfang HINTER der Kamera liegt (bei B
  Blick nach Osten/Norden), sortiert der Randstreifen hinter das Visier, wird danach gezeichnet und
  übermalt den Leuchtpunkt dort, wo er im Bild liegt (das Glas schreibt keine Tiefe, der
  Visierkörper schon -> nur das Reticle verschwindet). Gleiches galt für jede Welt-Transluzenz mit
  Ursprung hinter der Kamera und für Pool-Effekte (Translation = Weltursprung).
- Fix (viewmodel.rs): Komponente FirstPersonDraw(Ebene); world.rs spawn_mesh setzt sie für jedes
  Mesh mit 1P-Flag (objects[].w = 1, beide Material-Pfade). Render-Welt: extract_fp_draws sammelt
  die sichtbaren 1P-Entities, fp_sort_last setzt in RenderSet::PhaseSort vor
  sort_phase_system::<Transparent3d> deren distance = 1e5 + Ebene -> die 1P-Transluzenz kommt immer
  nach aller Welt-Transluzenz (wie UE3 SDPG_Foreground als eigener Durchgang). Untereinander bleibt
  die Reihenfolge wie bisher (alle 1P-Teile hatten schon dieselbe Translation). Mündungsfeuer des
  Spielers (WeaponEffectMuzzleFlash View1P/ViewADS-Vorlagen, mark_fp_muzzle; Vorlagen, die auch
  als 3P-Feuer der Bots dienen, bleiben Welt) Ebene -1: nach der Welt, vor Glas/Reticle.
- NOT from data: Basis 1e5 (größer als jede Welttiefe in m), Ebenen 0 / -1, Muzzle-Zuordnung über
  die Pools. Tiefen-Stauchung (gamemat.rs) unverändert; Welt-Transluzenz untereinander unverändert
  (Pool-Effekte am Weltursprung sortieren weiter ungenau gegeneinander – nicht geändert).
- Gebaut; im Fenster NICHT getestet. Szenario ads1 (Regen, Mittag, Zielen per HOMEBOTS_TEST_ADS,
  Spieler auf dem Südrand der Straße 21 m östlich von B, Blick nach Osten entlang des Streifens,
  Spline-Anfang hinter der Kamera): Leuchtpunkt muss über dem Randstreifen sichtbar bleiben.
  Zu prüfen am PC: außerdem Zielfernrohr-Glas (ACOG) über Wasser/Pfützen, Mündungsfeuer beim
  Schießen vor Rauch/Pfützen sichtbar, Waffe weiterhin vor Wänden.

## Kollision neben der kleinen Brücke (Crossroads) + Kollisions-/Wireframe-Ansicht im Debugmenü (2026-10-21, Nutzer 22:00)
- Ursache: Straßen-Splines. Die Straße über die Bachbrücke westlich der Autobahn (Crossroads_terrain
  RoadSpline_13, roadSection1_noBottom, ScaleX 2.5, Start UE -5522,1377; Bach bei ~-3000,1400 mit
  highwayGaurdRailStraightShortConcrete) hat im Akteur DisableCollision=True und CollisionScaleX=0.
  main.rs gab jedem RoadSpline die Dreiecke des gezeichneten Meshes als Kollision (inkl. Randstreifen
  und M_TransparentRoad-Sektion, ~16 m breit) -> die Fahrbahn ragte links und rechts über die Brücke
  in die Luft über dem Bach. Daten: Engine.u Default__RoadSpline CollisionScaleX 1.0, RoadSpline-
  MeshComponent Start/EndCollisionScale (Kopien der CollisionScaleX an den Stößen; RoadSpline_5
  endet an der Brücke mit End = nicht gesetzt = 0, RoadSpline_13 Start nicht gesetzt).
- Fix: upk level.rs RoadSplineMesh.collision (= !DisableCollision) und collision_scale
  (Start/End; nicht gesetzt = 0, beide nicht gesetzt = CollisionScaleX des Akteurs); world.rs
  road_collision: dieselbe Verformung wie road_mesh, Breite (Mesh-X) mal lerp(Start, End) und nur
  Sektionen mit EnableCollision (neu upk mesh.rs static_mesh_section_collision; alle Straßen-
  Sektionen haben es gesetzt). NOT from data / nicht in der Exe geprüft: dass CollisionScale die
  gezeichnete Breite multipliziert.
- Betroffen (keine statischen Mesh-Platzierungen; nur Straßen-Splines): Crossroads und Overpass je 4
  Segmente (RoadSpline_13 ohne Kollision, _5 läuft zur Brücke hin auf 0 aus, _14/_15 am Stoß 0,7),
  Angel Island 16 sichtbare Segmente (6 DisableCollision ohne HiddenGame, 10 mit 0,5..1,3). Die
  Straßen liegen sonst auf dem Terrain, dort ändert sich nichts Spürbares. pa.rs (mark_road) kennt
  RoadSpline_13 dadurch nicht mehr als Straße.
- Geprüft und NICHT die Ursache: Kollisions-Meshes (examples/colcheck, neu: Abstand jedes
  Kollisions-Vertex zur LOD0-Oberfläche; Crossroads max. 1,35 m an cliff_convex_02, sonst <0,8 m und
  innen liegend), BodySetup-/nicht dekodierte Meshes (nehmen die gezeichneten Dreiecke),
  BlockingVolumes (Daten, keine Instanz-Flags), GCVehicleBlockingVolume (nicht in der Spieler-
  Kollision). Die kaputte Autobahnbrücke über denselben Bach (UE ~0,-4000) ist es nicht.
- Debugmenü F1 -> Werkzeuge -> "Kollision / Wireframe" (neu coldebug.rs, NOT from data, Dev-Tool):
  Spieler-Kollision (farbig nach Quelle: grün Kollisions-Mesh, gelb Render-Dreiecke statt BodySetup,
  magenta LOD-Layout nicht dekodiert, cyan Render-Dreiecke nach Mesh-Regel, rot BlockingVolume,
  braun GCVehicleBlockingVolume (nur Anzeige), blau Skeletal, weiß Straßen/sonstiges, grau Terrain
  optional), Schuss-Kollision (orange), Wireframe der gezeichneten Meshes, Radius-Schieber
  (5-120 m, Neuaufbau bei 1 m Bewegung, max. 400k Linien), Name am Fadenkreuz (Mesh-Pfad/Volumen,
  Quelle, Spieler/Schüsse, Entfernung; auch bei geschlossenem Menü). Gespeichert in
  homebots_settings.ini (Debug:*). Linien als LineList-Mesh mit unlit StandardMaterial (bevy_gizmos
  ist nicht im Build), leicht zur Kamera versetzt. Kein echter GPU-Wireframe (POLYGON_MODE_LINE
  müsste beim Start als wgpu-Feature angefordert werden; fehlt es, startet das Spiel nicht).
  player.rs CollisionWorld führt dafür je Dreieck Quelle + Besitzer (SRC_*, new_owner, tri_info).
- Test: HB_DEBUG_COLLISION=1 schaltet Spieler-Kollision + Fadenkreuz-Name beim Start ein (=all auch
  Schüsse + Wireframe). Szenario cb1 (Bachufer 8,6 m südwestlich der Brücke, Blick auf die Brücke).
- Gebaut; im Fenster NICHT getestet (Linien-Darstellung, Tiefenversatz, FPS auf der iGPU, ob die
  Minimap-Kamera die Linien mitzeichnet). Neue Dev-Tools: examples/colcheck (auch --below <z> für
  eine Terrain-Karte unter der Wasserhöhe, --near x z r), secflags; roadactors zeigt jetzt die
  Kollisions-Flags der Spline-Komponenten.
- Nachtrag 23:56 (Nutzer): Leuchtspur-Licht und -Leuchten nochmals um 20 % gesenkt (Faktor ×0,8 auf beides): nachts jetzt ×0,64, tagsüber ×0,48 des Ausgangswerts. NOT from data – Nutzerwunsch.

## Abgefallene Autotüren schweben nicht mehr (2026-10-21, Nutzer 21:46)
- Neu `piecephys.rs`: einfacher Starrkörper für abgefallene Teile (Map-Autos: Türen/Haube/Kofferraum; Fahrzeug-Wrackteile): Schwerkraft aus Daten (Default__WorldInfo DefaultGravityZ 750 × RBPhysicsGravityScaling 2.0), Kontaktpunkte = Extrempunkte der Teil-Vertices (gedreht), Sweep gegen Wände + Bodenkontakt je Punkt, Impulse mit Reibung/Restitution, Kippen bis flach liegend, Schlafen. Die eigene Kollision des Autos wird beim Bodentest übersprungen (vorher landeten Türen auf der unsichtbaren Auto-Kollision bzw. am nicht gedrehten tiefsten Punkt und hingen in der Luft). Masse/Reibung/Restitution/Dämpfung/Schlafschwellen NOT from data.

## Zerstörbare KVA-Lastwagen (Angel Island) (2026-10-22, Nutzer 23:56 "this truck should also be destructible")
- Daten: der Lastwagen ist ein gewöhnlicher StaticMeshActor mit StaticMesh VEH_military.trucks.Statics.NK_truck
  (Fahrerhaus + Fahrgestell, Front = -Y, Glas-Sektion M_NK_truck_glass_C), meist mit eigenem Aufbau-Akteur
  NK_truck_back_01 / NK_truck_back_02 / truckMissiles. Nur auf Angel Island: 14 Lastwagen (Barracks 2, Command 2,
  Communication 4, Hospital 5, Maintenance 1), Crossroads/Overpass haben keine Militär-Lastwagen (nur HumV_Debris).
  Keine Knochen, Sockets, Schadensschwellen, kein PhysicsAsset/zerstörtes Mesh in den MP-Daten.
- SP-Vorlage GMS_ExplodingCars trucks.NKTruck benutzt GENAU dieses Mesh (InterpActor NKTruck_9Arc0). Übernommen:
  Leben 1000 (SeqEvent_TakeDamage_1), Explosion KFX_Pyrotechnics.FX_Explosions.Vehicle_Jeep am Spawnpunkt
  NKTruck_22Arc0 (im LKW-Rahmen (1,1, 157,1, 86)), SC_LightVehicleAnnihilation_SP, CauseDamageRadial 1000 / 1024 /
  Abfall / GCDmgType_Veh_HeliMissile (wie die Autos über combat.rs), Wrackfeuer Fire_WreckChassis am Wrack
  NKTruck_21Arc0 + AttachToActor Main_Root z 12,75 (im LKW-Rahmen (0, 173, 50,75)).
- NICHT in den gelieferten Daten: das SP-Wrack SP_NK_MilTrucks.SKV_NK_MilTruck / PA_MilTruck_wreck und die
  *_Wreck-MICs / MI_CH_Z10_glass_DEST (in den fehlenden SP-Karten gekocht) -> kein Mesh-Tausch; der LKW bleibt
  stehen (Kollision bleibt), "verbrannt" = Abdunklung wie bei den Autos (gamemat objects[].z = 5), auch der
  Aufbau. SP versteckt am Wrack Attach_TroopCover/CattleGuard/Missiles_Rear – hier bleiben die Aufbauten (nur dunkel).
- Umgesetzt (levelcars.rs StaticTruck/TruckBed, update_trucks in update_cars; main.rs statische Platzierungen
  über levelcars::static_kind/static_shape/spawn_static): Schüsse auf die Box der Mesh-Grenzen (8 uu Rand),
  Explosionen über Abstand zur Box (einmal je Explosion, Kettenreaktion mit Autos/anderen LKW), Glas zerspringt
  (Glas-Sektion dieser Platzierung ausgeblendet + WindowExplode_Side + SC_Debris_Glass), Motorbrand, Explosion,
  Wrackfeuer, Brand-Loop, Abdunklung. Trucks stehen in der HB_TEST_CARHITS-Liste.
- NOT from data: Glas bei 150 Schaden nahe der Scheiben (Fenster-DestroyThreshold der Jiggle-Autos, 30 uu um die
  Glas-Box), Motorbrand bei 500 (Fire_Car, Position 18 % hinter der Front, 45 % Höhe), Burndown 5 s (TruckCab1;
  NKTruck hat keinen), Schaden = Grid-Wert PM_Default, Treffer-Box mit Rand, Zuordnung Aufbau -> nächster LKW
  (<= 450 uu), Main_Root des Wracks am Ursprung angenommen. Nicht umgesetzt: SeqEvent_TakeDamage_0
  (GCDmgType_MD600_Seat0_Missile >= 100 sprengt sofort; Explosionen tragen hier keinen Schadenstyp zu levelcars),
  RB_RadialImpulse 75000/384, GCAftermathTrigger, LensFlare, Wrack-Abwurf.
- Neues Dev-Tool crates/upk/examples/smsections.rs (Material + Grenzen je Sektion eines StaticMesh).
- Szenario tr1 (Barracks-LKW StaticMeshActor_402, Spieler 14,5 m, viewspot dir 60). Gebaut; im Fenster NICHT
  getestet. Zu prüfen am PC: tr1 (2 s Glas weg, 6 s Feuer am Fahrerhaus, 11 s Explosion + Wrackfeuer, LKW und
  Aufbau dunkel, Feuer-Positionen plausibel, Spieler überlebt).

## Hubschrauber lassen sich nicht mehr aufrichten; Panzer F2-Sicht und Fadenkreuz (2026-10-22, Nutzer 23:56)
- Heli auf der Seite (AH-64 ~80° nahe am Boden): Ursache in heli_step (vehicles.rs) – die Roll-Grenze aus der EXE
  (GCVehicleSimChopperBase::UpdateVehicle 0x0071cd36..0x0071ce43, nur bei CollectiveMultiplier < 0,7, Spieler 0,12)
  schneidet die Roll-Eingabe ab, wenn die Lage jenseits lerp(sin fAHMinRoll 30°, sin fAHMaxRoll 45°, CM) ≈ 31,7° liegt
  UND die Eingabe weiter nach außen drückt. Bei uns war das Vorzeichen vertauscht: abgeschnitten wurde die Eingabe
  ZURÜCK zur Waagrechten, die nach außen nie -> der Heli rollte über ~66° (Auftriebsrichtung zeigt nach unten, dort
  keine Auto-Nivellierung, Eingabe nur 25 %) und war nicht mehr zu retten. Jetzt richtig herum.
- Nick-Grenze fehlte ganz: EXE 0x0071cec1..0x0071cf8d schneidet die Nick-Eingabe genauso jenseits lerp(sin fAHMinPitch
  40°, sin fAHMaxPitch 45°, CM) ≈ 40,6° ab; Zusatzdämpfung +100 (DAT_013805b4) außerhalb -0,9 < sin(Nick) < 0,9, bei
  abgeschnittener Eingabe immer (vorher: +100 ab der Grenze). Konstanten aus der EXE gelesen (0,7 / 0,25 / 0,2 /
  0,85 / 0,05 / 0,4 / 0,5 / 2,0 / ±0,9 / 100). Auto-Nivellierung (RollUprightFactor / PitchUprightFactor,
  Fenster 0,05 < t < 0,85, Roll nur ohne Roll-Eingabe oder jenseits sin 0,4, x2 bei CM <= 0,3) war schon wie in der EXE.
  Daten: AH64 RollUpright 1500, PitchUpright 500, RollTorqueStrafe 350, Max 25000; MD600 RollUpright 1000,
  RollTorqueStrafe 200, Max 10000; fAHMin/MaxRoll 30/45, fAHMin/MaxPitch 40/45 (GCVSimChopper_AttackHeli / _MD600).
  bAlwaysUpright bleibt false (keine StayUpright-Federn für AH64/MD600).
- NOT from data: außerhalb des EXE-Fensters (t >= 0,85, ab ~40° Schräglage, Überschwingen / Kollision) wirken die
  Upright-Momente weiter und die Achse ohne Eingabe nivelliert mit HELI_LEVEL 2,0 /s² je rad (≈ 0,8 x Winkel pro s).
  fNegativeRollClamp/PitchClamp (FUN_0061a2d0 spiegelt Werte über Max mit einem Zufallsfaktor zurück, Daten 0)
  nicht nachgebaut: einfache Klemmung. PhysX-Körperdämpfung weiter angenähert (HELI_ANG_DAMP).
- Prüfstand: HB_SLOPETEST=heli HB_HELIROLL=1 (3 s voll rechts rollen: AH64 bleibt bei ~36°, dann zurück; Nase 2 s
  voll hoch) bzw. =2 (bei 8 s auf 80° gekippt, ohne Eingabe zurück). Ergebnis: Rollen voll rechts endet bei ~36° (AH64) /
  ~34° (MD600) statt weiterzukippen; Gegensteuern wirkt sofort; 80° -> 41° in 1 s, 20° nach 2 s, danach im
  EXE-Fenster langsam (~2,5°/s) weiter; Nase voll hoch überschwingt auf 43-54° und fällt ohne Eingabe auf ~30° zurück.
  Nur im Prüfstand, nicht im Fenster.
- Panzer F2 (Sitz 1, MG/Kommandant): TurretParentSeatIndex = 0 (M1A3 und LAV Sitz 1, Daten) – MG_ROTATE hängt im
  Skelett unter Turret_Rotate. Unser Sitzwinkel ist relativ zum Rumpf (Zielrichtung, Kamera), die Pose drehte ihn
  aber zusätzlich mit dem Hauptturm -> MG, Mündung und das Innen-Mesh Interior_US_M1A2_Gunner (MGViewSocket auf
  MG_PITCH (-34; 0,7; 22,3)) zeigten um den Turmwinkel daneben; die Kamera stand im Innenraum und sah auf die
  Seitenwand ("TARGETING ASSISTANCE"-Schild). Jetzt: Knochen unter dem Eltern-Turm drehen um (Sitzwinkel -
  Elternwinkel); SeatCfg.turret_parent aus TurretParentSeatIndex.
- bCameraFixedToSocket1P (M1A3 Sitz 1 true, Sitz 0 false): die 1P-Sicht solcher Turmsitze schaut jetzt entlang des
  Turms (Socket), der der Maus mit seiner Drehrate folgt (M1A3-MG 60°/s, AH64-Bordkanone 50°/s) – vorher lief die
  Sicht dem Turm samt Innen-Mesh voraus. Pro Sitz wird nur das eigene Cockpit-Mesh gezeichnet (Sitz 0
  Interior_US_M1A2, Sitz 1 Interior_US_M1A2_Gunner; war schon so). Glas-Meshes (m_GlassMesh /
  m_SecondaryGlassMesh, bUse(Secondary)GlassMesh1p) weiter nicht gezeichnet.
- Fadenkreuz Fahrersitz (F1, Sitz 0 = Hauptkanone): VehiclePlacerTank.Seat1Placer (SeatIndex fehlt = 0).Reticule
  (11 Bilder Reticule_Tank_Circle / Vertical- / HorizontalDash, Positionen absolut 1280x720: Kreis 624,344 Größe 32 =
  Bildmitte) hat als einziges Fahrzeug-Reticule ein Widget-Rechteck 0,305..0,695 x 0,361..0,639. ui.rs verschiebt die
  Bilder eines GCUIObject_Placer um dessen Ecke -> Fadenkreuz bei (1030, 620) über dem Fahrzeug-Symbol. Jetzt: alle
  GCUIObject_Placer unter einem GCUIObject_VehicleSeatPlacer werden absolut gezeichnet (hud.rs seat_reticules).
  Sitz 1 nutzt Seat2Placer (SeatIndex 1).Reticule (Kreis + Striche, war schon sichtbar).
- Offen / ungeprüft: Bild im Spiel nicht angesehen (nur gebaut); Lage/Drehung des MGViewSocket gegen das
  Gunner-Innen-Mesh nicht gerendert geprüft; T99 (KVA) nicht geladen; Kamera-Lag, Glas-Meshes.

## Menü-Audit + Team wechseln (2026-10-22, Nutzer 16:22 "Buttons im ESC-Menü nicht richtig texturiert, ich kann nicht das Team wechseln")
- Neues Werkzeug crates/upk/examples/uishot.rs: rendert eine UI-Szene ohne Spielfenster auf der CPU als PNG
  (Texturen + Fonts, Canvas-PS pow(tex_lin*vcolor, 1/2,2); Materialien nur als dunkler Platzhalter, fehlende
  Texturen magenta). `uishot <HOMEFRONT> <Szenenpfad> 1366 768 out.png`, HB_STATE=Tag:Focused,
  HB_SHOW/HB_HIDE=Tag, HB_TEXT=Tag=Text. Zusätzlich styletags.rs (welcher Skin definiert welchen StyleTag).
  Ohne Laufzeitwerte (Listen, Regler-Zahlen) – die füllt erst das Spiel.
- Audit (Szene -> Befund -> Fix), geprüft per uishot/uidraw bei 1366x768:
  - Pausemenü PC_U_PauseMenu_MP -> "ABSTIMMUNG AUSRUFEN" (CallVoteButton) mit dem grau-grünen
    EngineResources.DefaultTexture-Karo -> Ursache: Stile ohne AssignedStyleID werden über DefaultStyleTag
    gesucht, und bei doppelten Tags gewann DefaultUISkin.DefaultSkin (alphabetisch zuerst) statt des aktiven
    PC_Skins.PCskin (abgeleiteter Skin). Jetzt gewinnt PCskin (ui.rs Ui::new). ButtonBackground ->
    Buttons.ButtonFeature-off wie die übrigen Knöpfe. Betrifft auch: Spawnauswahl "Trupp-Browser"
    (SquadMenuButton), Grafik "Übernehmen" (ApplyChangesButton), Tastenbelegung-Reiter, KeyBindConflict-Popup
    Ja/Nein, Scrollbar-/Listen-Stile (DefaultIncrement/DecrementButtonStyle, DefaultScrollBarStyle,
    ListCell*TextStyle: dadurch zwei vorher verdrehte Rechtecke in Tastenbelegung/HUD-Einstellungen richtig).
    Gegenprobe: Hauptmenü, Mehrspieler, Spiel erstellen, Popups: Zeichenliste unverändert (trace-geprüfte
    Szenen bleiben gleich).
  - Tastenbelegung PC_U_KeyBindings -> mittlerer Reiter ohne Text -> Markup "<STrings:...TabLandVehicles>"
    (großes T); Loc::resolve nimmt das Präfix jetzt ohne Groß-/Kleinschreibung.
  - Tastenbelegung -> Reitertexte riesig (Skalierung 1, Stil des Knopfs) -> UITabButton nimmt jetzt den
    TabButtonCaptionStyle seines UITabControl (Skalierung 0,5, Farbe 0,30, Autoscale). Dessen StylePadding -25
    schob den Text links aus dem Reiter; das TabButtonPadding des Controls (5,0 PercentViewport), das das
    wohl ausgleicht, ist nicht verstanden -> negatives Padding bei Reitern auf 0 (NOT from data).
    TabButtonBackgroundStyle und der Zustand des gewählten Reiters (UIState_TargetedTab) NICHT umgesetzt.
  - Spawnauswahl -> "TEAM WECHSELN" (SwitchTeams, GCCommand_UISwitchTeam) war ohne Aktion = nicht klickbar
    -> jetzt Team wechseln (unten).
  - Punktetabelle PC_U_InGameScoreboard (Tab) -> gab es gar nicht -> neu (scoreboard.rs, unten).
  - Ohne Befund (Darstellung = Daten): Hauptmenü, Mehrspieler, Spiel erstellen, Optionen-Übersicht,
    Grafik/Audio, HUD-Einstellungen, Message-Popup, Kill-Cam-Szene. Pausemenü-Abstimmungsfeld (Vote) ist
    offline ohne Funktion (wie bisher).
  - Nicht geprüft: Hover/Pressed-Zustände im laufenden Spiel (nur Focused per uishot an CallVoteButton),
    Texte "Initial Label Text" in Wertefeldern (werden zur Laufzeit gefüllt), PC_U_Controls (Konsolen-Szene,
    nicht erreichbar).
- Team wechseln (wie im Original: Spawnauswahl-Knopf "TEAM WECHSELN", UI_MenuText SpawnSelect SwapFaction;
  im Pausemenü über "AUSRÜSTUNG WECHSELN", dort steht derselbe Knopf):
  - combat.rs player_team()/set_player_team() (vorher war der Spieler überall fest TEAM_USA). Ersetzt in
    combat/airstrike/drone/vehicles/destruction (Treffer, Freund/Feind, Granaten-Team), bots.rs (Ziel Spieler,
    Feuer auf Spieler, Feindliste, Wärmebild), minimap.rs (Kameraden), objectives.rs (Eroberung),
    player.rs (Spawnpunkte: Frontlinien-Sammlung des Teams, sonst Infantry_Low/VehicleSpawnsLow für KVA),
    nuke.rs player_team() leitet weiter, debug.rs (Gegner/Team töten relativ).
  - Tot (Spawnauswahl): sofort neue Spawnauswahl fürs andere Team (Flagge USA/Korea-Placer, Fahrzeugliste
    GCV_CH_* statt GCV_US_* aus VehicleCosts), Countdown läuft weiter, Ausrüstung bleibt, Fahrzeug = Infanterie.
    Lebend: Tod wie "Selbstmord", dann Spawnauswahl/Spawn im neuen Team.
  - Ablehnung mit UI_MenuText Popups CannotSwap ("ZUGEHÖRIGKEIT KANN IM MOMENT NICHT GEWECHSELT WERDEN") +
    OkButton nur, wenn keine Runde läuft oder "Auto. Team-Ausgleich" an ist und keine Bots da sind.
    NOT from data: FL2GameMP.ChangeTeam (Teamgrößenprüfung mit GCOnlineServerOptions, PendingTeamSwap-Paare)
    nicht dekodiert; Wechsel im Leben zählt als Tod.
  - Bots (bots.rs manage_bots): das Spielerteam bekommt want/2 Bots, das andere den Rest; nach dem Wechsel
    verlässt ein Bot das neue Team (tote zuerst) und einer kommt im freien Slot ins alte. main.rs: 8 Körper-Slots
    je Team (vorher 7 USA + 8 KVA). NOT from data (MP hatte keine Bots).
  - HUD (hud.rs): Farben relativ - eigenes Team = USA-/Freund-Farbe der Daten, Gegner = KOR-Farbe (Ziele,
    Minimap/Karte, Kill-Crawl); obere Punktezeile = eigenes Team (Name, Punkte, Runden). Die Punktetabelle
    der Daten macht es genauso (Friends links mit Friends{US,KPA}FlagImage, Enemy rechts) - für den HUD selbst
    NOT verified.
  - Test-Hook HB_TEST_SWITCHTEAM=<s>: s Sekunden nach jedem Spawn "AUSRÜSTUNG WECHSELN" -> "TEAM WECHSELN".
  - Offen: 1P-Arme und Kill-Cam-Körper bleiben US; KVA-Fahrzeuge für den Spieler nicht gefahren/getestet.
- Punktetabelle (scoreboard.rs): PC_U_InGameScoreboard solange EKBC_OpenScoreboard (Tab, DefaultInput
  BindingsPCGlobal) gehalten wird; eigenes Team links, Flaggen nach Team, Zeilen Name/Punkte/Kills/Tode in den
  Kopfspalten, Spielerzahl in Friends/EnemyNumberLabel. NOT from data: Zeilenschrift GamerTagFont 0,6,
  Zeilenhöhe = ListSelectionImage-Höhe, Spielerzeile grün, Punkte = Kills x 10, Sortierung nach Punkten.
- Nicht im Spiel angesehen (nur gebaut + uishot); der Nutzer testet selbst.

## Leistung: Profiler-Log 2026-10-22 16:21 (perf_1792678343, Angel Island, 180 s)
- Befund: 50 FPS, Frame 20,0 ms (p95 28,6). Engpass ist der Render-Thread (render app 18,0 ms, Frame ≈ Render + ~2 ms
  Extract; main app 13,0 ms, nur in 4 % der Frames länger als Render). render_system 11,3 ms (Befehlsaufzeichnung:
  command_buffer_generation_tasks 6,8 ms, drei Schatten-Kaskaden + Haupt-/Transparent-Pass bei ~4900 sichtbaren von
  18 576 Mesh3d), allocate_and_free_meshes 2,7 ms bei 67 geänderten Mesh-Assets pro Frame (40 Partikel-Emitter,
  ~9 Bot-Teile, 1P-Waffe, Drohne) + extract_render_asset<RenderMesh> 0,7 ms (Klon jedes geänderten Meshes im Extract,
  blockiert beide Threads). Main: render_bots 1,9, update_drone 1,35 (2,6–3,6 ms solange die Drohne draußen war),
  check_visibility 1,3, check_entities_needing_specialization je Material ~0,7 (9 Materialien; par_iter-Zeiten
  enthalten fremde Aufgaben, die der wartende Thread abarbeitet – nicht additiv lesen).
- Gegenüber 2026-10-21 (perf_1792575880, 70 FPS): +3300 Entities, doppelt so viele Mesh-Uploads (33 -> 67), neu die
  Drohne; render_system 8,8 -> 11,3 ms (ohne GPU-Zeitstempel nicht weiter aufteilbar; in der Drohnen-Phase 13,5 ms).
- Spitzen: sfx 116 ms beim Absetzen der Drohne (t = 77 s: Schleifen-Wellen werden synchron dekodiert, sfx_decode
  57 ms + Paket/Cue-Laden), coldebug::rebuild bis 26 ms (Kollisions-Overlay war an: Neuaufbau bei jedem Meter),
  bot_plan bis 22 ms, render_system-Ausreißer 37–55 ms (Treiber/Present).
- Geändert (alles gleiches Bild / Verhalten, NOT from data):
  - drone.rs update_drone: lief jedes Frame mit sichtbarer Drohne über alle ~18 600 Mesh3d-Entities und suchte für
    jede in allen Drohnen-Meshes (≈ 750 000 Vergleiche). Jetzt über den Index MeshEntities (≈ 30 Lookups), gleiche
    Vorrang-Regel (Körper/Anbauteil vor Cockpit); `shown` wird erst gesetzt, wenn alle Drohnen-Meshes indiziert
    sind. Häuten von Körper/Anbauteilen/Cockpit und das Ketten-UV-Scrollen nur noch, wenn sich Pose, Lage, Drehung,
    Rotor-Skalen bzw. Kettenweg geändert haben (abgestellte / stehende Drohne: 0 statt ~6 Mesh-Uploads pro Frame).
    HB_DRONEREF / HB_DRONEDBG einmal gelesen. Erwartung: update_drone 2,6–3,6 -> ~0,3–1 ms mit Drohne.
  - viewmodel.rs write_skinned (Bots, Drohne, 1P-Arme/Waffe): schreibt in die vorhandenen Attribut-Puffer statt vier
    neue Vecs pro Mesh und Frame (~29 000 Vertices/Frame); render_bots/viewmodel etwas schneller (~0,1–0,3 ms).
  - particles.rs fill_mesh: Vertex-Arrays werden übernommen statt kopiert; bei gleicher Vertexzahl bleiben Index-
    Puffer und die konstanten Tangenten stehen (vorher je Emitter und Frame neu: 7 Vecs). HOMEBOTS_PARTICLE_DEBUG
    einmal pro Frame statt pro Emitter gelesen. Erwartung particles 0,6 -> ~0,4 ms.
  - menu.rs UI-Batches (HUD): ein Batch mit denselben Quads (inkl. Drehung) und demselben Material wie im letzten
    Frame behält sein Mesh; vorher bekam jeder Batch bei jeder HUD-Änderung ein neues Mesh (Upload, neue Slab-
    Zuteilung, Mesh2d-Neuspezialisierung). Nicht während des Lade-Spinners. Erwartung: weniger Uploads im
    Render-Thread (je unverändertem Batch einer), menu::update ~0,5 -> ~0,3 ms.
  - bots.rs render_bots (kleine lokale Änderung): HB_BOTCAM einmal gelesen (static BOTCAM) statt per
    std::env::var für jeden neu gehäuteten Bot in jedem Frame.
- Geschätzt gesamt: Main-Thread −2 bis −3 ms mit Drohne, −0,5 ms ohne; Render-Thread −0,3 bis −0,8 ms (weniger
  Uploads: Drohne + HUD). Die großen Posten (render_system, Uploads von Partikeln/Bots) brauchen strukturelle
  Änderungen: GPU-Skinning für Bots/Drohne/1P, GPU-Billboards für Partikel (ein fester Puffer statt Mesh-Neubau),
  bindless GameMat (weniger Bind-Gruppen-Wechsel), Schatten der „full level“-Kaskade für statische Geometrie
  zwischenspeichern.
- Offen / Empfehlungen: Drohnen-Sounds (Schleifen) beim Kauf oder Levelstart vorladen (sfx preload), damit das
  Absetzen nicht 116 ms hängt; Kollisions-Overlay (F1 Werkzeuge) im Normalbetrieb aus lassen (bleibt in
  homebots_settings.ini gespeichert); menu.rs same() vergleicht die Drehung (rot) der Quads nicht. Nicht im Spiel
  gemessen (kein Fenster); neue Aufnahme mit HB_PERF=1 zum Vergleich.

## Bots: Können wie in einem echten Online-Match (2026-10-22, Nutzer 16:21 "Make all bots more skilled", Bot-Log 1792678337)
- Befund Bot-Log 1792678337 (Engelsinsel, 307 s live, 154 Snapshots): Tode USA 48 / KVA 35; nur 39 % der
  lebenden Bot-Zeit mit Ziel; mit dem nächsten Gegner unter 10 m griffen nur 70 % an, 10-20 m 51 %,
  20-30 m 39 % (Sichtkegel +-60°, kein Hören); Deckung 3 % der Zeit; 20x "no progress", davon viele
  Bots, die unterwegs kämpften und dann zum nächsten Ziel geschickt wurden.
- Daten gefunden (GCGame.u, SP-KI; das MP-Spiel hatte keine Bots, daher nur angepasst genutzt):
  GCAIConfig.BotMemoryConfig (SightProcessMin/Max 0,25/0,9 s, NoiseProcessesTimeMin/Max 0,1/0,4 s,
  NoiseProcessesDistanceMax 3000), BotDeviationConfig (InitialDeviationDeg 30, SecsToFullyAccurate 10 –
  SP-Werte, NICHT übernommen), BotFireConfig (Bot/TargetIsMovingModifier 0,75, SimpleFireDistance 1800,
  Prozent-Trefferchancen 1/5 % = SP-Würfelsystem, NICHT übernommen), BotAnticipationConfig
  (Distance_MinForAnticipation 200), Default__GCBotConfig Weapon0 (Stationary Wait 0,375-0,675 /
  Duration 0,675-1,375 s, Moving Wait 2,5-6 / Duration 0,5-1, OpenFullFireDistanceMax 500, Reload
  MaxBurstTime 1,75), Default__GCAIEngagementConfig (Targeting-Punkte, fDuckAfterShotByBot/PlayerChance
  0,5/0,8, Grenades: Cooldown 4-10, AreaCooldown 3,5/AreaRadius 1000, AllyRadius 200, Wurfweite 120-2000,
  Höhe +-200, ThrowChanceHidden 80-100 / Visible 5, MinTargetStationaryTime 1, ReEval 1 s, MaxSWATRange 800),
  BotHealthConfig RegenRate 10 (wie bisher). Die SP-Schwierigkeitsobjekte FL2SPDifficulty* liegen nicht in
  den MP-Paketen (nur Namen). Werkzeuge: examples/objfull, structdef, propgrep.
- Neu `crates/homebots/src/bots/brain.rs` (Untermodul von bots.rs, Haken in bot_ai):
  - Bot-Stärke = Optionen "Schwierigkeit" für BEIDE Teams: EINFACH/NORMAL/HART/GUERILLA = 0,35/0,6/0,8/1,0,
    Standard jetzt HART (settings.rs bot_level, Default Difficulty 2), KVA +0,05 (alter Wunsch "Gegner etwas
    stärker"); HB_BOTSKILL=<0..1> überschreibt. team_skill() liefert weiter einen Faktor (1 + 0,75 x Stufe),
    den botextras/botvehicles nutzen.
  - Reaktion: Daten 0,25-0,9 s auf Stufe 0, 0,18-0,35 s auf Stufe 1 (Wunsch), x0,6 wenn erwartet (dort
    gehört / Gegner dort verloren), +0,08 s am Rand des Sichtfelds. Drehrate 6-11 rad/s nach Stufe.
  - Zielen: Fehler beim Erfassen 2,5-6° + 5 % des Schwenkwinkels, setzt sich mit tau 0,9-0,3 s,
    Restzittern 0,9-0,22° (Ornstein-Uhlenbeck), schlechter bei eigener/Ziel-Bewegung (1/0,75 aus Daten);
    Zielpunkt Brust..Hals nach Stufe (Kopfanteil der Treffer ~10-20 %), Ziel-Geschwindigkeit mit 0,12 s Verzug,
    Vorhalt 30-85 % nach Stufe; Rückstoß Recoil Hip/Zoom ViewPitchMin..Max, davon 50-85 % ausgeglichen,
    Erholung ViewRecoveryRate; Waffenstreuung wie beim Spieler (Spread Min + FireAdd bis Max, DecayRate;
    Kegel Spread/2, -95 % mit ADS). ADS ab 6 m (ADSBringUpTime/BringDownTime), Laufen dabei mit
    ADSMovementPercent. Ersetzt die alte Streuung "1,2 + 4 x settle + Bewegung + Stand.Min/2".
  - Feuerstöße nach Entfernung (GCBotConfig): unter 5 m Dauerfeuer bis 1,75 s, sonst Stationary/Moving
    Duration (gedeckelt durch BotMaxFireRate), ab 18 m kürzer (Antippen), Pausen Stationary-Wait bzw.
    beim Laufen zwischen Moving-Wait (Stufe 0) und Stationary (Stufe 1); Halbautomatik 0,3-0,16 s Takt.
  - Zielwahl BotTargetScoringConfig (VisibleBonus, Entfernungszonen mit _Human-Faktoren, Beschossen 1000,
    aktuelles Ziel 50/s bis 4 s) statt "nächster"; die 5 nächsten Kandidaten werden geprüft.
  - Hören: Schüsse von Bots und Spieler (brain::noise / player_noise) im Umkreis 3000 uu, wahrgenommen
    nach 0,1-0,4 s je Entfernung -> Bot dreht sich dorthin. Letzte bekannte Position 4 s angeschaut.
  - Blickrichtung ohne Ziel: Geräusch / letzte Position / Laufrichtung, im Stehen alle 1,5-3,5 s über
    offene Sichtlinien (16 Strahlen bis 25 m, Richtung gegnerische Spawns bevorzugt) – kein Starren auf Wände.
  - Deckung: Chance aus Daten 0,5 (Bot) / 0,8 (Spieler) statt 35/65 %, beim Vorstoß x0,6, unter 50 HP
    mind. 0,85; Dauer 3-5 s, verletzt 6-9 s (Regeneration); in Deckung abwechselnd geduckt (nachladen
    unter 60 % Magazin) 1-2 s und stehend spähen 1,5-3 s.
  - Im Gefecht stehend auf > 12 m in einem Teil der Gefechte geduckt; Zickzack beim Laufen unter
    Beschuss / beim Vorstoß mit Feindkontakt; Sprint schon ab 15 m Weg und 1,5 s ohne Feindsicht.
  - Granaten (bot_grenades): 2 je Leben (Ammo der Frag-Daten), Regeln aus BotGrenadeConfig (siehe oben),
    Wurf wie beim Spieler (Speed + TossZ, Zünder), Abwurfwinkel per Flugbahn-Suche; Log "throws a grenade".
  - Ziele: wird ein eigener Punkt erobert (capturing_team = Gegner), laufen Wächter und jeder zweite
    andere Bot zur Verteidigung; im Zielvolumen halten Bots einen Punkt 2,5-5,5 s (statt alle 1-3 s
    weiterzulaufen); "no progress" zählt nicht mehr, solange der Bot kämpft.
  - Bot-Log: Snapshot-Zeilen zusätzlich "kd K/D crouch true/false ads 0..1"; Spiel-Log nennt die Stärke.
- Alles außer den oben genannten Datenwerten NOT from data (Formeln, Schwellen, Stufenverläufe).
- Offline-Schätzung (Python-Nachbau des Zielmodells, M4-artig, stehendes/seitlich laufendes Ziel):
  Stufe 0,8 – Trefferquote 8 m ~60-68 %, 20 m ~33-35 %, 35 m ~21-25 %, 50 m ~16-18 %; Kill-Zeit Median
  0,7 s (8 m) / 1,05 s (20 m) / 1,8 s (35 m). Stufe 1,0 deutlich tödlicher (20 m ~0,75 s), Stufe 0,35 ~2x
  langsamer. Kein Aimbot: erste Schüsse nach dem Schwenk gehen meist daneben.
- Nicht umgesetzt: Wechsel zur Pistole (Loadout-Code gehört zum Killstreak-/Fahrzeug-Teil), Nahkampf,
  Ecken-Checks beim Laufen (nur Blick auf Geräusche/letzte Position), Flankieren außer dem bestehenden
  Umweg-Punkt bei weiten Zielen.
- Gebaut; im Spiel NICHT getestet. Zu prüfen am PC: Gefechte auf Engelsinsel (Reaktion, Treffer auf
  20-40 m, ob Bots zu stark/schwach sind -> Schwierigkeit in den Optionen), Granaten, Bots in Deckung
  (ducken/spähen), Verteidigung eines eingenommenen Punkts, neues Bot-Log (kd-Spalte).

## Bots: Kampfpunkte, Killstreaks, Fahrzeuge, Raketenwerfer (2026-10-22, Nutzer 16:21 "Bots should use killstreaks, use vehicles, and use rocket launchers to shoot down vehicles")
- Neue Module botextras.rs (KP, Killstreaks, Werfer, Schadensfilter) und botvehicles.rs (Fahrzeuge); bots.rs
  nur zwei Haken: bot_ai überspringt Bots, die botextras steuert (`controlled`: Werfer in der Hand / im
  Fahrzeug; Weg wird danach neu geplant), render_bots zeichnet Insassen nicht (`hidden_slot`). HB_BOTX=0 schaltet
  alles ab (Bots wie vorher). HB_BOTKP=<n>: Start-KP der Bots (Tests, z. B. 5000 = sofort Fahrzeuge/Luftschläge).
- Kampfpunkte pro Bot (BotKit), wie beim Spieler: Start GCBattlePoints InitialBattlePoints 500, Obergrenze
  BattlePointsCap 7000, Kill = KillBP der Streak-Stufe (TDM: tdm::kill_bp), PSR_CaptureObjective 250 für die
  Bots im Volume beim Besitzerwechsel, PSR_DestroyVehicle 250 (FL.Infantry Rewards) für den Zerstörer eines
  Bot-Fahrzeugs - auch für den Spieler (Banner "PSR_DestroyVehicle"). KP bleiben über den Tod erhalten.
  Kosten aus SpecialAbilityCosts / VehicleCosts. NICHT umgesetzt: Kopfschuss-Bonus, PSR_AdvanceFrontline,
  Assists für Bots.
- Ausrüstung: das DefaultLoadout, dessen Primärwaffe der Bot trägt (Items 4/5: Splitterweste+PZF3, Weste+
  Bodendrohne, Hellfire+PZF3, Bodendrohne+Hellfire, Weste+UAV, Luftdrohne+PROX).
  - Splitterweste (CFG_EQP_FlakJacket 250): einmal pro Leben, Leben 100 x MaxHealthScale 1,5 (die Regeneration
    füllt weiterhin nur bis 100 auf, NOT from data). Fahrer-Bots behalten 300 KP Reserve (NOT from data).
  - UAV (CFG_EQP_UAV 400, MaxSweepDuration 30 s): wenn das Team keinen laufen hat, 25 %/s sobald bezahlbar (NOT
    from data). Wirkung fürs Team (NOT from data: geteilt): Team des Spielers -> Gegner auf seiner Minimap
    (hud.uav_left); botextras::team_uav(team) für bots.rs abrufbar (dort noch nicht benutzt) und erweitert die
    Luftschlag-Zielwahl auf die ganze Karte.
  - Luftschläge (CombatData::strikes, Kosten aus den Daten): Bots mit dem Hellfire-Item nehmen nach Slot
    Hellfire / Streubombe / Weißer Phosphor / Sensorgezündet (NOT from data: die Standard-Loadouts haben nur
    Hellfire; steht für eigene Loadouts). Ziel: die Stelle mit den meisten Gegnern (Fahrzeuge zählen doppelt)
    im Radius der Waffe (Bomben: Streufläche, Hellfire: DamageRadiusFull/Radius), mindestens 2, innerhalb
    100 m vom Bot (mit Team-UAV überall). Die UAV kreist wie beim Spieler (orbit FlyAltitude/FlyRadius/FlySpeed,
    Motor-Sound der UAV-Klasse an ihrer Position), schießt nach 3 s Anflug (NOT from data) ihr Magazin im
    ReloadTime-Takt auf den Gegner nächst zum Zielpunkt (NOT from data: nachgeführt); Bomben über payload wie
    beim Spieler (Bomblets, Phosphor usw.). Abklingzeit 30 s pro Bot, 20 s pro Team (NOT from data).
  - Drohnen (GAssault/GAntiTank/AAssault/ARecon), Wärmebild, Nachschub: für Bots NICHT umgesetzt (drone.rs ist eine
    einzelne Spieler-Drohne); einmal im Bot-Log vermerkt.
- Raketenwerfer gegen Fahrzeuge (Loadout-Item CFG_ATW_PZF3 / CFG_ATW_PROX): Feindfahrzeug in Sicht (Sichtlinie zur
  Box-Mitte, keine andere Fahrzeugbox davor) und Reichweite -> kaufen (250 KP, PurchaseAmount 3 Raketen) falls
  leer, ziehen (EquipTime), zielen, feuern, nachladen (ReloadTime), weg (EquipTime). PROX: Lock-on nach
  LockTime (das JAV-LockOn, main.rs) -> Rakete mit Seek wie beim Spieler; PZF3: Vorhalt (zwei Iterationen,
  gerade Flug m_fGravityFactor 0) nach 0,7-1,4 s / Skill. NOT from data: Reichweiten (PROX MaxLockDist, höchstens
  100 m; PZF3 60 m, gegen Hubschrauber 40 m), Zielwahl (nächstes, Hubschrauber und < 30 m zählen halb),
  Zielfehler 1-2,5° / Skill, Abbruch nach 1,5 s ohne Sicht. Der Bot hält dabei weiter das Gewehr-Modell (3P-
  Werfer-Mesh nicht gezeichnet). Bots im Fahrzeug, leere Fahrzeuge und eigene Fahrzeuge sind keine Ziele.
- Fahrzeuge (botvehicles.rs):
  - KVA-Fahrzeuge werden jetzt mitgeladen: vehload::KVA_CLASSES GCV_CH_M1114 / LAV / T99 / MD600 / Z10_Content
    (VehicleCosts, gleiche Kosten wie USA; vehload blendet bei CH-Klassen die US_*-Teile aus).
    vehload::all_classes() in main.rs (Laden + Fahrzeug-Raketen). HB_NOKVAVEH=1 lässt sie weg (Ladezeit).
    Damit hat auch ein KVA-Spieler (Team wechseln) kaufbare Fahrzeuge - ungetestet.
  - Kauf beim Respawn (wie der Spieler in der Spawnauswahl): jeder dritte Bot eines Teams ist "Fahrer" (NOT from
    data); kauft das teuerste bezahlbare Fahrzeug seines Teams zu 65 %, sonst ein anderes; nur einen HUMVEE zu
    50 % (spart sonst). Höchstens 3 Bot-Fahrzeuge und 1 Hubschrauber pro Team gleichzeitig (NOT from data). Von
    den 2 Instanzen einer Klasse des Spielerteams bleibt eine für den Spieler frei; verlassene Bot-Fahrzeuge
    werden bei Bedarf recycelt; spawn_vehicle recycelt Bot-Fahrzeuge zuletzt.
  - Spawn: SpawnPoints der aktiven Front vom Typ der Klasse (spawn_type), sonst GMST_Jeep, sonst
    VehicleSpawnsHigh/Low (Hubschrauber "#air"), mindestens 9 m von anderen Fahrzeugen.
  - Fahren (Bodenfahrzeuge, vehicles::ground_step wie beim Spieler): Ziel wie goal_for (nächster offener Punkt der
    Front, sonst nächster Gegner, sonst gehaltener Punkt), Weg über das Lauf-Gitter (nav.rs, jeder 4. Knoten),
    Vorausschau bis zum fernsten sichtbaren der nächsten 6 Wegpunkte, Lenkung 2,5 x Winkelfehler, 80 % der
    Höchstgeschwindigkeit, Panzer drehen bei > 70° auf der Stelle, ab 12 m vom Ziel bremsen und dort stehen
    (alles NOT from data). Feststecken: 2 s langsam trotz Gas -> 1,5 s rückwärts mit Gegenlenkung; 15 s ohne 3 m
    Fortschritt (nach 2 Versuchen) -> aussteigen ("abandons ... stuck").
  - Hubschrauber (NOT from data, kinematisch statt heli_step: dessen Flugmodell braucht Steuer-Eingaben): 4 s
    warten auf einen Schützen, senkrecht steigen auf 25 m über Grund (auch vorausschauend 1,5 s), mit 60 %
    AirSpeed zum Ziel, dort Kreis mit 35 m Radius, Nase zum Ziel des Piloten (feste Waffen), sonst in
    Flugrichtung mit MaxYawRate; Rumpf-Kugeln gegen die Welt wie bisher; unpiloted_fall lässt bot-geflogene
    Hubschrauber in Ruhe. Hubschrauber-Bots steigen nicht aus (außer 15 s am Boden festgeklemmt).
  - Schützen: Teamkameraden steigen in wartende (Spawn) oder langsame Fahrzeuge (< 150 uu/s) in einen freien
    Sitz mit Waffe: frisch gespawnt bis 30 m (80 %), sonst bis 10 m (50 %), Wächter nicht (NOT from data; der Weg
    zum Fahrzeug wird übersprungen). Fehlt der Fahrer, rückt ein Schütze auf Sitz 0. Steigt der Spieler ein
    (E / F1 / F2), weicht der Bot auf einen freien Waffensitz aus oder steigt aus; fährt ein Bot und der Spieler
    sitzt am Geschütz, bewegt nur botvehicles das Fahrzeug (vehicles.rs drive: Haken `bot_moves`).
  - Waffen der Sitze: Zielwahl alle 0,3 s (Feindfahrzeuge nur für Projektil-Modi, mal 0,4 bevorzugt; Soldaten /
    Spieler zu Fuß in Sichtlinie und Waffen-Reichweite), Turm mit fTurretYaw/PitchMaxSpeed und den Höhen-Grenzen
    TurretControllerMin/MaxLimit, Feuer innerhalb 3° (MG) / 2,5° (Granate/Rakete) / 8° (feste Hubschrauber-
    Waffen), Reaktion 0,6 s / Skill, MG-Feuerstöße 1,6 s an / 0,7 s aus, +1° Streuung / Skill (alles NOT from data).
    Sofort-Treffer wie drive (Schadensraster, Kopf/Brust, Splash, Leuchtspur, Einschlag, Überhitzen, Loop-Sound an
    der Mündung), Projektile wie drive (Magazin, Intervall, Nachladen, Mündungsfeuer; lock-on-Modi suchen das
    Zielfahrzeug), Start außerhalb der eigenen Box (Bot-Raketen ignorieren das eigene Fahrzeug nicht).
    Motor-Loop (SCEngineIdle) an der Fahrzeugposition.
  - Aussteigen: Bodenfahrzeug unter 30 % Leben -> alle springen raus (NOT from data), Ausstieg am
    Seat<n>StartPosition-Socket.
  - Schaden: Insassen nehmen keinen eigenen Schaden (filter_damage zwischen update_grenades und apply_damage);
    wird das Fahrzeug zerstört (verschwindet <= 1,5 s nach einem Treffer), sterben sie mit (Kill für den letzten
    Angreifer, mit dessen Icon). Fahrzeuge eines Teams nehmen keinen Schaden vom eigenen Team (FriendlyFire aus,
    wie bei Soldaten). Infanterie-Kugeln gehen weiter nicht durch die Boxen und schaden Fahrzeugen nicht (wie
    bisher).
- Bot-Log (gleiches Format "[Zeit] Name ..."): KP-Gewinne (Kill, Eroberung, Zerstörung), Käufe (Weste, Werfer,
  Fahrzeug mit Ort), Killstreaks (UAV, Luftschlag mit Ziel/Wertung, jeder Schuss, Ende mit Kills), Werfer (ziehen,
  Schuss mit Entfernung / Vorhalt / Lock, Treffer auf Fahrzeug bzw. Spielerfahrzeug, Ziel verloren), Fahrzeuge
  (fährt zu Ziel X, steigt ein als Schütze, Sitzwechsel, steigt aus mit Grund, verlassen/geparkt, zerstört von X
  mit Toten, Schütze greift Fahrzeug/Spieler an, Granate/Rakete auf Fahrzeug).
- Sichtbarkeit geändert (nur pub(crate)): bots::bot_ai, combat::update_grenades/apply_damage, vehicles::wrap/
  body_rot/seat_dir/flash_fx/update_volumes/empty_vehicle_damage, VehicleKind::pose/socket_world.
- Offen / ungeprüft: nur gebaut, nicht im Fenster getestet (Nutzer testet). Ungeprüft insbesondere: ob die
  KVA-Fahrzeuge alle laden (T99/Z10-Meshes, Wracks) und wie lange das Laden dadurch dauert; Fahrzeug-Wegfindung auf
  dem Infanterie-Gitter (enge Gassen -> Feststecken/Aussteigen erwartet); Hubschrauber-Flughöhe über Gebäuden;
  Turm-Vorzeichen bei Sitzen auf einem Eltern-Turm (turret_parent); die Bot-Infanterie (bots.rs) zielt weiter mit
  Gewehren auf Insassen von Fahrzeugen (Kugeln prallen ab); das Lock-on des Spielers erfasst auch eigene
  Bot-Fahrzeuge (kein Team-Test, wie vorher bei leeren); Insassen werden nicht gezeichnet.

## Atomschlag beendet das Match + zufälliges Wetter je Runde (2026-10-22, Nutzer 16:21 / 16:22)
- Wünsche: "Randomize time and weather on each round startup." / "Remove the Nuke class. Instead hand over
  the killstreak to one bot or the player, depending on which team wins, before match end. Freeze the point
  count and let the nuke basically end the round." Alles hier NOT from data (das Spiel hat weder Atomschlag
  noch Wetter).
- Nuke-Klasse entfernt: kein Loadout ATOMSCHLAG mehr (7. Eintrag im Einsatzmenü, wMenuLabel6-Kopie,
  CFG_Nuke im Spawn-Menü und in den Loadouts von main.rs/spawnsel.rs/menu.rs raus; nuke.rs with_loadout /
  spawn_loadout / add_spawn_label gelöscht). Das F1-Debugmenü (Killstreaks > ATOMSCHLAG gratis) und
  HB_NUKETEST bleiben als Test.
- Regel (objectives.rs Endgame, Konstanten oben im Abschnitt "end-of-match nuke"):
  - Punkte: in der Runde, die das Match entscheidet (gewonnene Runden des Teams + 1 = nötige Runden;
    TDM 1, Bodenkontrolle 2), erreicht ein Team 90 % des Punktelimits (END_SCORE_FRAC, Test:
    HB_ENDNUKE_FRAC=<anteil>) -> dieses Team bekommt den Atomschlag.
  - Zeit: Rundenzeit <= 30 s (END_TIME_LEFT) und ein Team führt -> der Führende bekommt ihn; das Match endet
    dann damit, auch wenn es nach Runden noch nicht entschieden wäre. Bei Gleichstand läuft die Zeit weiter
    (auch über 0) bis ein Team führt.
  - Frühere Runden der Bodenkontrolle enden wie bisher (Front rückt, Banner RUNDE GEWONNEN/VERLOREN).
  - Ab der Vergabe ist eingefroren: Teampunkte, Rundenzeit, Eroberung/Verfall der Ziele, Halte-Punkte;
    Kills zählen keine Teampunkte (combat.rs fragt objectives::scores_frozen()). KP des Spielers laufen weiter.
  - Spieler im Gewinnerteam: Banner "ATOMSCHLAG BEREIT: TASTE 4 (n)" + Crawl-Zeile; die Taste des ersten
    Spezial-Slots (EKBC_SpecialWeapon1, Name aus der Belegung) ruft ihn statt des Loadout-Items (combat.rs
    nuke::offered_to_player). Nach 20 s ohne Anforderung (END_PLAYER_GRACE; auch wenn tot/im Fahrzeug)
    ruft ein Bot seines Teams. Ablauf dann wie bisher (Zünder, Fulton-Ballon, Flugzeug, Mittelfinger, ...).
  - Gegnerteam gewinnt: nach 3 s (END_BOT_DELAY) ruft ein lebender Bot des Gewinnerteams (sonst irgendeiner
    des Teams, sonst "USA"/"KVA") – Crawl "<Name>: ATOMSCHLAG ANGEFORDERT". "Bot-Modus" in nuke.rs: Countdown im
    Banner ("FEINDLICHER ATOMSCHLAG IM ANFLUG", eigener Bot: "TAKTISCHER ..."), Sirene, Sprengkopf,
    Detonation, Wolke, Krater, Weißblende, danach der Fernblick auf die Wolke; kein Ballon/Flugzeug, keine
    Arm-Animationen, kein God-Mode, eigene Sicht bis zur Weißblende (Radial-Blur-Zentrum aus der
    Spielerkamera). Die Druckwelle tötet wie bisher alle Bots darin und jetzt auch den Spieler (Treffer vom
    Bot, im Fahrzeug über das Fahrzeug; Unverwundbar-Cheat schützt). Bis zur Detonation respawnt der
    Spieler normal, danach nicht mehr.
  - Treffer der Druckwelle kommen vom Aufrufer (Spieler oder Bot, Team = Gewinner), nicht mehr fest vom
    Spieler/USA.
  - Ende: 7 s nach der Detonation (WIN_AT) -> Endgame Done -> das Gewinnerteam bekommt die nötigen Runden,
    Banner SIEG/NIEDERLAGE (GCUIScene_WinLossSplash YourTeamWon/Lost, relativ zum Spielerteam) 8 s, dann wie
    bisher zurück ins Menü. Ein neues Match = neue Karte + neues Wetter. Der alte Trick (Punkte des
    Spielerteams auf das Limit setzen) ist raus, der Atomschlag kann das Ergebnis nicht mehr ändern.
  - menu.rs: flow.leave wird jetzt auch in der Spawnauswahl ausgewertet (Match vorbei, während man tot ist).
  - Test: HB_ENDNUKE=<s>[,<team>] vergibt ihn nach s Sekunden Spiel (Team 0 USA / 1 KVA, Standard = Team des
    Spielers); Log-Zeilen "endgame: nuke to team ...", "nuke: called by bot ...", "endgame: match over".
- Zufälliges Wetter / Tageszeit (weather.rs round_weather): bei jedem Matchstart (sobald die Karte gebaut ist,
  noch hinter Ladebildschirm/Film, sofort ohne Überblendung) und bei jeder neuen Runde (Rundenzähler ändert
  sich; Wetter blendet in 8 s über, die Sonne läuft in 8 s vorwärts zur neuen Uhrzeit). Nicht beim Matchende.
  Gewichte: Klar 50 %, Regen 15 %, Gewitter 10 %, Schnee 10 %, Nebel 15 %; Uhrzeit 70 % Tag 8–17 h, 15 %
  Dämmerung 5–8 / 17–20 h, 15 % Nacht 20–5 h; Tageszeit steht (keine Tag-Nacht-Uhr). Log: "round weather:
  match start: Rain, 14:37 h" / "round weather: round 2: ...". Im Front-End wieder aus (Original-Look).
  - Option: Optionen > Spiel (U_HUDPrefs), die Zeile "Logitech LCD" (ohne Funktion) ist jetzt
    "Zufälliges Wetter/Tageszeit" (Schlüssel HomeBots:RandomWeather in homebots_settings.ini, Standard an;
    "Standard wiederherstellen" setzt sie wieder an). Aus = das Wetter bleibt wie im Debugmenü eingestellt.
  - HB_WEATHER / HB_TIME / HB_DAYNIGHT gesetzt -> keine Zufallswahl (feste Testwerte wie bisher).
  - Bekannt: der Gewitter-Himmelsglanz (FX_Lightning) wird weiter nur mit HB_WEATHER gepoolt
    (Weißer-Himmel-Verdacht vom 2026-10-21) -> zufälliges Gewitter hat Blitzlicht + Donner, aber keinen
    Himmels-Glow.
- Gebaut; im Fenster NICHT getestet (der Nutzer testet selbst). Zu prüfen am PC: HB_ENDNUKE=20 (Spieler-
  Prompt, Taste 4), HB_ENDNUKE=20,1 (KVA-Bot ruft, Spieler stirbt in der Druckwelle, Fernblick, NIEDERLAGE),
  Länge des Optionstexts in der Zeile (Breite ~230 px bei 1280), Wetterwechsel bei Rundenende.

## Alle Multiplayer-Karten (2026-10-22, Nutzer 16:21 "Start porting all Multiplayer maps")
- Vorgehen: je Karte den Ordner vom PC des Nutzers in /mnt/user-data/uploads gestaged (nur .map,
  _LOC_deu/_LOC_int, *_package.upk, Lightmaps; nicht ins Repo), HB_MAPCHECK=<Ordner> ohne Fenster,
  generische Fehler behoben. Nichts im Fenster gestartet (der Nutzer testet selbst).
- HB_MAPCHECK prüft neu auch den Level-Bau ("== world build", mapcheck.rs world_check; HB_MAPCHECK_NOWORLD=1
  schaltet ihn ab): Pakete wie load_maps (Sublevels, *_FX, Install-Pakete nicht exportierter Texture2D/
  Material/MIC-Importe), Auflösung + Lesen aller StaticMesh-/SpeedTree-/SkeletalMesh-Platzierungen
  (nicht lesbare StaticMeshes namentlich), Emitter/Wasser/Straßen, Importe aus Paketen, die im Install
  fehlen (im Container ist das Install unvollständig -> die Liste dort nur als Hinweis), Zensus der
  Actor-Klassen mit Mesh-Komponente, die kein Platzierungspfad liest, WGSL aller platzierten Materialien
  (world.rs build_material: Graph-Compiler + naga) mit gestubbten Knoten, Terrain-Spielshader je Palette
  (wie spawn_terrain_emu). Jeder Schritt unter catch_unwind (Panic wird gemeldet statt abzubrechen).
  Dazu "spawn data complete" (jede Front hat Infanterie-Starts für beide Teams). Skeletal-Platzierungen
  mit Mesh aus dem Komponenten-Archetyp werden wie in load_maps aufgelöst. Alle 13 Karten-Ordner: 0 Panics.
- Generische Fixes (gelten für alle Karten):
  - Ziele/Fronten (objectives.rs load_pkgs): nur Objective_GameMode_VolumeHold, die ein
    GameMode_FocusedFrontline desselben Pakets listet. Tiefland: lowlands_church.map enthält eine
    verwaiste Kopie von "A" (gleiche Position) -> vorher 10 Ziele, und im TDM wurde sie zur eigenen
    "Front" -> 0 TDM-Starts.
  - Linien = die FocusedFrontlines selbst (line_ranks), nicht mehr "nach X sortieren, je 3": Hafen
    (GRÜNE ZONE) läuft entlang Y (vorher mischte die mittlere Linie Ziele dreier Fronten -> 0 Starts),
    Sackgasse und Großkaufhaus haben 2 Ziele je Front (vorher 2 Linien à 3 -> 0 Starts), Grenzland hat
    nur 1 Front mit 2 Zielen. Reihenfolge NOT from data: Achse = Summe (KVA-Infanterie-Schwerpunkt -
    USA-Schwerpunkt) je Front, Linie 0 = USA-Ende (ein USA-Rundensieg schiebt +1). Kreuzung/Engelsinsel/
    Bauernhof: dieselben Linien wie vorher (geprüft). Fallback X-Regel nur, wenn ein Ziel keiner Front
    zugeordnet ist.
  - Persistente Karte (main.rs level_files): "fl-<Ordner>" zuerst (Hafen hat zusätzlich
    fl-harbor-suburb / -terrain / _StripMall, Abflusskanal FL-Spillway-sky/-Terrain/-FrontlinesData ohne
    Unterstrich).
  - Spielmodus-Sublevel (mapmode.rs): TDM auch "tdm-..." (Sackgasse: tdm-culdesac_gastation), Bodenkontrolle
    auch "...-FrontlinesData" (FL-Spillway-FrontlinesData); main.rs level_files nutzt dieselben Regeln.
  - GCRotatingSMActor (Windrad-Rotoren u. ä., StaticMeshComponent + RotationSpeed; Bauernhof 31,
    Tiefland 9, Engelsinsel 7) wird jetzt platziert (upk level.rs static_mesh_placements), statisch -
    die Drehung (RotationSpeed) ist NICHT animiert.
  - StaticMesh-Leser (upk mesh.rs read_static_mesh): ein LOD-Header, der nicht parst, beendet die
    LOD-Liste (die LODs davor bleiben, Vertexdaten per Suche im ganzen Export) statt den ganzen Mesh zu
    verwerfen. Einige Kopien derselben Meshes in manchen Sublevels haben nach dem LOD-0-Header andere
    Felder (Office_Small_TwoStory in borderlands_bases: 3141 Vertices, danach 2, 5856, Indexdaten statt
    der 7 dwords) -> LOD 1 las Müll (720906 Sections) -> Mesh fehlte. Betroffen und jetzt lesbar
    (Bounds = Extent des Exports geprüft): Grenzland Office_Small_OneStory/TwoStory (27 Gebäude!),
    Hafen VEH_military NK_truck (58), Bauernhof Warehouse_A_Foundation_1280 (38) + whitePhosCrate01 (25),
    Engelsinsel indFence4/indFenceBroke (37 Zäune). Neu dazu read_static_mesh_any: Meshes ohne
    Inline-LOD (alles separate Bulk-Daten) lädt read_static_mesh_full aus dem Quellpaket.
    Dev-Tools: crates/upk/examples/smunread (nicht lesbare StaticMeshes je Paket), smhdr (Header-Dump).
- Kartenliste (creategame.rs): alle GCUIInfo.m_MPMapsInfo-Einträge (15: Basic, ANGELISLAND, BIGBOX,
  BORDERLANDS, CROSSROADS, FARM, HARBOR, LOWLANDS, SPILLWAY, SUBURB, Culdesac, Bridge, MainStreet,
  Alcatraz, Mining), deren Ordner für mindestens einen Modus Spieldaten hat; vorher nur die 8 aus dem
  Trace (TRACE_MAPS entfernt; NOT the original filter). Basic/MainStreet/Mining haben keinen Ordner,
  Suburb (VORSTADT) nur suburb_package.upk -> nicht gelistet. Ohne MapTextureName (Bridge, Alcatraz)
  Bild aus UI_FrontEndTextures_DLC2.MPLoading.<MapName>. EXTRA_MAPS neu: TDMSpillway = "WASSERWEG"
  (ui_frontend.MapSelect.TDMSpillway_Name, Bild ...DLC2.MPLoading.TDMSpillway, nur TDM). "spielbar" je
  Spieltyp weiter aus den Dateien (mapmode::has_data) plus mapmode INCOMPLETE: alcatraz/FL (siehe unten).
- Neues Dev-Tool crates/upk/examples/mpmaps (m_MPMapsInfo aller GCUIInfo-Objekte).

| Karte (Ordner) | Name | Modi | Status (HB_MAPCHECK, Endstand) | Bekannt / offen |
|---|---|---|---|---|
| Farm | BAUERNHOF | GC, TDM | GC 3 Fronten x 3 Ziele, Start USA/KVA 16/17; TDM 20/19; 4170 Meshes (vorher 63 fehlend), 869 Mat. WGSL ok, Terrain 3201² 4/4 Paletten | Windräder (GCRotatingSMActor) stehen still |
| Lowlands | TIEFLAND | GC, TDM | GC 3x3, 22/20; TDM 25/25 (vorher 0: verwaistes Ziel); 3608 Meshes, 961 Mat. ok | – |
| Harbor | GRÜNE ZONE | GC, TDM | GC 3x3 (Linien entlang Y), 17/19 (vorher 0); TDM 25/24; 3941 Meshes (vorher 60 fehlend: NK_truck), 896 Mat. ok | keine Fluid-Wasserfläche in den Daten |
| Culdesac | SACKGASSE | GC, TDM | GC 3x2 Ziele, 16/16 (vorher 0); TDM 19/19 (vorher nicht erkannt: tdm-...); nur Infanterie | 3 Materialien ohne Kompilat (nicht gezeichnet) |
| borderlands | GRENZLAND | GC, TDM | GC 1 Front x 2 Ziele, 12/10; TDM 15/14; 3946 Meshes (vorher 27 Bürogebäude fehlend), 1016/1017 Mat. | nur eine Linie (Rundensieg verschiebt nichts) |
| Spillway | ABFLUSSKANAL | GC | GC 3x3, 18/18 (vorher nicht erkannt: -FrontlinesData); 903/904 Mat., Wasser 1, 152 Emitter | 6 FX_MeshLight-Lichtkegel (StaticMesh-Import aus Nicht-Kartenpaket, wie bisher nicht gezeichnet) |
| TDMSpillway | WASSERWEG | TDM | TDM 15/14; 3141 Meshes, 926/927 Mat., Terrain 4/4 | eigener Ordner, eigener Listeneintrag; FX_MeshLight wie Abflusskanal |
| BigBox | GROSSKAUFHAUS | GC, TDM | GC 3x2, 22/20; TDM 20/20; 3516 Meshes, 510 Mat. ok | nur Infanterie; Geschäft = StaticMeshActors (kein BSP: keine ModelComponent auf irgendeiner Karte) |
| bridge | BRÜCKE | GC, TDM | GC 3x3, 26/17; TDM 28/27; 3851 Meshes, 1075/1076 Mat., Terrain 4/4 | – |
| alcatraz | ALCATRAZ | TDM (GC aus) | TDM 12/12, nur Infanterie; 2923 Meshes, 293/295 Mat. | GC-Daten unvollständig (6 PlayerStarts für 3 Fronten, Front 0/2 ohne Starts) -> mapmode INCOMPLETE |
| Suburb | VORSTADT | – | nur suburb_package.upk, keine .map | nicht im Install, nicht gelistet |
| Crossroads | KREUZUNG | GC, TDM | unverändert (gleiche Linien, 17/17; TDM 22/22) | – |
| AngelIsland | ANGEL ISLAND | GC | 18/24; 3113 Meshes (vorher 37 Zäune fehlend), 7 Rotoren platziert | – |
| Overpass | HIGHWAY | TDM | unverändert 15/15 | – |

- Fahrzeuge: alle 5 kaufbaren Fahrzeug-Meshes in den Paketen jeder Karte gefunden (GCGameContent).
- Nicht geprüft (nur Daten, kein Fenster): Rendering/Licht, Bot-Navigation (nav grid wird beim Laden
  generisch gebaut), Kollision, Minimap-Bilder, Ladezeit/Speicher großer Karten (Bauernhof 3201²-Terrain).
  PA-Masten weiter nur Kreuzung.

## Nachtrag zu "Bots: Kampfpunkte, Killstreaks, Fahrzeuge, Raketenwerfer": Skill-Skalierung (2026-10-22)
- bots::team_skill liefert jetzt für beide Teams 1 + 0,75 x Stufe (bots/brain.rs; Standard HART 0,8 -> 1,6, KVA
  +0,05). Werfer- und Fahrzeug-Bots teilten ihre Zeiten/Fehler direkt durch team_skill (ausgelegt auf 1,0/1,25)
  und wären dadurch ~60 % genauer/schneller geworden. Jetzt botextras::skill_factor = team_skill / 1,6,
  begrenzt auf 0,7..1,15 (NOT from data): bei Standard-Schwierigkeit gelten die Grundwerte (Werfer zielen
  0,7-1,4 s, Fehler 1-2,5°; Schützen Reaktion 0,6 s, +1° Streuung MG / +0,5° Projektil), EINFACH ~0,8 (langsamer,
  ungenauer), GUERILLA ~1,12.

## Atomschlag nur vor dem Matchsieg (2026-10-22 19:21)
- Nutzer: "das Ganze match. Die nuke soll quasi gegeben werden kurz bevor ein team das ganze match gewinnt."
- `objectives.rs`: Die Zeitregel (≤ 30 s Restzeit, ein Team führt) vergibt die Nuke jetzt auch nur in der Runde, mit der das führende Team das Match gewinnt (gewonnene Runden + 1 ≥ benötigte Runden; TDM 1, Bodenkontrolle 2). Vorher hätte sie schon in der ersten Bodenkontrolle-Runde das Match beendet. Die Punkteregel (90 % des Punktelimits) galt schon nur in der entscheidenden Runde. Frühere Runden enden wie bisher (Frontlinie verschiebt sich). NOT from data (das Spiel hat keine solche Nuke).

## Bunter Bildschirm, Blaustich auf Bridge, Kollisions-Overlay (2026-10-22 19:43)
- Nutzer (Screenshot Bridge): "my whole screen spazzes out in rainbow colors ... fix the video and day/nighttime issue. also the wireframe was on by default but should be off".
- Bunte Linien über dem ganzen Bild: das Kollisions-Overlay (F1 > Werkzeuge > "Kollision / Wireframe", Dreiecke farbig nach Herkunft, Neuaufbau bei jedem Meter) war an und blieb über homebots_settings.ini von Start zu Start an (Log: die ini wurde 9 s nach dem Spawn mit allen Schaltern auf 0 gespeichert = im Spiel ausgeschaltet; der Profiler-Log von 16:21 zeigte es schon an). `coldebug.rs`: alle Overlays starten jetzt bei jedem Programmstart aus (auch "Fahrzeug-Volumen", vorher Standard an); nur der Radius bleibt gespeichert. HB_DEBUG_COLLISION=1 schaltet weiter für Tests ein.
- Blaustich / "wie Nacht" auf Bridge (Runde: Nebel, 10:45 Uhr): verwendet wurde immer das PostProcessVolume mit der höchsten Priority der ganzen Karte. Bridge (FL-Bridge_postprocess.map) hat zwei: PostProcessVolume_1 (Priority 1, unter z -1196 = unter Wasser: TintMidTone 0,07/0,24/0,87, Entsättigung 0,4) und PostProcessVolume_2 (Priority 0, bis z 56675: grau 0,59, Entsättigung 0,05). Jetzt wie UE3: das Volumen, in dem die Kamera steht (höchste Priority unter den enthaltenden), `main.rs` post_volume_select + `upk level.rs` post_process_volumes (Brush-Grenzen). Enthaltensein über die Bounding-Box des Brushs (die Volumen sind Quader; für andere Formen NOT from data); Wechsel sofort ohne Scene_InterpolationDuration (NOT from data). Außerhalb aller Volumen: die bisherige Wahl. Angel Island / Crossroads / Overpass haben je ein Volumen: unverändert.
- Schatten-/Tiefen-Shader von 33 maskierten Materialien (Ranken, Zäune, Kabel, Planen, ...) kompilierten nicht ("no definition in scope for identifier: HF_FLAG"): der Platzhalter der Ausbrenn-Färbung (levelcars.rs) wurde in prepass_source nicht ersetzt. Jetzt 0 (nur Farbe, für die Tiefe egal). Diese Objekte werfen wieder Schatten.
- Gebaut; im Fenster nicht getestet (Nutzer testet). Neue Dev-Beispiele: upk examples/volz.rs, ppvols.rs.

## Dauerndes Humvee-Geräusch (2026-10-22 20:17)
- Nutzer: "i keep hearing a looping humvee sound".
- Ursache: Der Motor von Bot-Fahrzeugen (`botvehicles.rs`) spielte die Cue SC_M1114_Engine2 ohne Schalter und ohne Lautstärken pro Ebene. So liefen alle Schleifen dauernd mit voller Lautstärke: Leerlauf, Reifen (NewHumV_Tires), Beschleunigen (JeepAccelerat) und Vollgas (NewHumV_Gas). Laut Cue sind sie bis 90 m (Leerlauf) bzw. 20 m (Gas) hörbar, also auch bei einem stehenden Bot-Humvee am Ziel oder am Spawn.
- Jetzt gelten dieselben Schalter und Ebenen-Lautstärken wie beim Motor des Spielers (vehicles.rs):
  - Schalter: "Gas", "BypassStartup" 0, "Sprinting" 0.
  - Tonhöhe: 1 + 0,2 × Drehzahl.
  - Ebenen-Lautstärken: Reifen nach Geschwindigkeit, Beschleunigen und Gas nach Gas.
- Gas kommt aus dem Bot-Gas: am Boden der Betrag des Gaspedals, beim Hubschrauber im Flug 1. Beim Warten oder ohne Ziel ist es 0. Es ist geglättet (4/s), damit kurzes Gaswechseln die Drehzahl-Einzelgeräusche nicht ständig neu abspielt.
- NOT from data: die Ebenen-Lautstärken über Tempo/Gas (wie beim Spieler).

## Bild blitzt rot / pink / grün / schwarz / weiß (2026-10-22 20:48, Video nach ~5 min Spiel)
- Im Video: Einzelbilder ganz rot, pink, schwarz oder weiß, teils nur die untere Hälfte rot mit scharfer Kante. Das ist das typische Bild eines nicht-endlichen Pixels (NaN / Inf) im HDR-Bild. Der Bloom-Weichzeichner und die 1×1-Augenanpassung verteilen so einen Wert über das ganze Bild, und die Kanäle klemmen dann zufällig auf 0 oder 1.
- `hfpost.wgsl`: Jeder Lesezugriff der Nachbearbeitung (Szene, Luminanz, Bloom, Anpassung) setzt NaN/Inf auf 0 und begrenzt auf 0–60000. Die Prüfung läuft über Bits, weil Compiler `x != x` wegoptimieren dürfen.
- `gamemat.rs` hf_final: Die Ausgabe jedes Spiel-Materials wird genauso bereinigt (16-Bit-Float-Ziel, höchstens 65504).
- `dynlights.rs`: `normalize(d)` ergab NaN, wenn eine Leuchtspur genau auf einer Fläche lag (d = 0). Jetzt wird mit `inverseSqrt(max(|d|², 1e-4))` gerechnet. Das ist ein möglicher Auslöser im Gefecht (bunte Leuchtspuren); die genaue Quelle ist nicht belegt. Die Bereinigung fängt jede Quelle ab.
- Alles NOT from data (Schutz der Nachbearbeitung). Geprüft: naga validiert hfpost.wgsl; HB_MAPCHECK Crossroads: 734 Materialien WGSL ok.

## Teamwechsel-Banner, Laufanimation der Bots (2026-10-22 20:43)
- Teamwechsel (`menu.rs` switch_team): Beim nächsten Spawn kommt das Banner "TEAM GEWECHSELT / DU BIST JETZT KVA" (bzw. USA), mit der langsamen Animation von GCUIBannerMessageManager (2,5 s, `objectives.rs`). Text NOT from data: das Spiel hat nur den Knopf "TEAM WECHSELN".
- Bots liefen rückwärts oder seitwärts, während sie vorwärts kamen.
  - Ursache: Ohne Ziel drehten sie sich 3–4 s zu jedem gehörten Schuss bzw. zur letzten bekannten Position (bots/brain.rs idle_yaw). Die Laufrichtung relativ zum Körper wählte dann run_back / run_left.
  - Jetzt schaut ein laufender Bot nur dorthin, wenn es höchstens 60° von der Laufrichtung abweicht, sonst in Laufrichtung.
  - Sprintende Bots (auch beim Sprint in Deckung) schauen in Laufrichtung, denn die Sprint-Animation läuft nur vorwärts. Vorher glitten sie seitwärts. Beim Sprinten schießen sie nicht.
  - NOT from data.

## Flackern: Leuchtspur-Licht an der Kamera (2026-10-22 21:18, Log + Video vom 20:48 genauer angesehen)
- Im Video sind einzelne Bilder rot, grün, gelb/weiß überstrahlt oder schwarz, mitten im Gefecht in einer Scheune. Einmal ist nur der Boden bis etwa 3 m rot, mit scharfer Kante, der Rest normal. Das passt zum Licht einer Leuchtspur (dynlights.rs, Radius 3 m, Helligkeit 4), das direkt an der Kamera sitzt: eigene Schüsse beim Verlassen des Laufs oder Bot-Leuchtspuren am Kopf vorbei. Es leuchtet Waffe, Boden und Wände im Umkreis von 3 m flächig rot, grün oder gelb an.
- Jetzt gibt es kein Leuchtspur-Licht innerhalb von 2 m um die Kamera und in den ersten 2 m nach der Mündung; ab 6 m volle Helligkeit (smoothstep). NOT from data.
- Das schwarze Einzelbild ist damit nicht sicher erklärt. Falls es bleibt: Log + Video.

## Bots: Granaten seltener, Killstreaks häufiger und sichtbar (2026-10-22 21:09 / 21:19)
- Granaten (bots/brain.rs bot_grenades), alles NOT from data:
  - Wurfchance × 0,35 (Daten: 80–100 % bei einem Ziel, das in Deckung ging).
  - Abklingzeit nach einem Wurf 10–20 s statt 4–10 s.
  - Gebietssperre 6 s statt 3,5 s.
  - Pro Team höchstens eine Granate alle 3 s.
- Luftschläge der Bots (botextras.rs killstreaks):
  - Schon 1 Gegner (Fahrzeug = 2) reicht als Ziel, vorher mindestens 2.
  - Gruppierungsradius mindestens 5 m, Reichweite 150 m statt 100 m.
  - Mit "2 Gegner innerhalb der 3–4 m der Hellfire" kam praktisch nie einer zustande.
  - NOT from data.
- Bot-UAV und Bot-Luftschlag erscheinen jetzt als Zeile in der Kill-Liste ("Name: HELLFIRE" / "STREUBOMBE" / "WEISSER PHOSPHOR" / "SENSORGEZÜNDET" / "UAV", in der Teamfarbe). NOT from data.
- Drohnen (Boden-/Luftdrohne aus den Standard-Ausrüstungen) benutzen Bots weiterhin nicht.

## Kein Regen / Schnee unter Dächern (2026-10-22 21:11)
- weather.rs update_precip_occlusion: Ein Gitter von 64×64 Zellen à 1 m um die Kamera hält die höchste Fläche jeder Säule. Dafür geht ein Strahl von 100 m über der Kamera nach unten gegen die Schuss-Kollision (Dächer, Brücken, Vordächer, Bäume mit Kollision).
  - Berechnung: nächste Zellen zuerst, 384 pro Frame; das Gitter wandert ringförmig mit der Kamera mit; neu bei einem Levelwechsel.
  - Daten: rgba32f-Textur mit Höhe, Zelle x, Zelle z und gültig.
- Der Regen-/Schnee-Shader lässt einen Tropfen weg, wenn er unter der Fläche seiner Säule liegt (Zellkennung geprüft, nicht berechnete Zellen = offen).
- Die Tropfen auf dem Bildschirm (screenrain.rs) wurden schon vorher unter Dächern reduziert.
- NOT from data (Homefront hat kein Wetter).
- naga validiert den Shader.

## Bots fahren Panzer und fliegen Hubschrauber (2026-10-22, Nutzer 21:32 "sie sollen auch drohnen nutzen und ebenfalls panzer und helikopter")
- Bisher kaufte ein Fahrer-Bot beim Respawn meist das teuerste Fahrzeug, das er gerade bezahlen konnte. Bei Start-KP 500 war das fast immer der Humvee; danach gab er seine KP für Weste/Werfer aus und kam nie auf Panzer- oder Hubschrauber-Preis.
- Jetzt hat jeder Fahrer-Bot eine Rolle (`botextras.rs` BotKit::role, `botvehicles.rs` Kauf):
  - Der 1. Fahrer eines Teams spart auf das teuerste Bodenfahrzeug des Teams (Panzer).
  - Der 2. Fahrer spart auf den Hubschrauber; das Limit von einem Hubschrauber pro Team in der Luft bleibt.
  - Die übrigen Fahrer kaufen wie bisher.
- Sparer kaufen nur ihre Klasse, sobald sie sie beim Respawn bezahlen können, und keine Splitterweste. Gibt es die Klasse für das Team auf der Karte nicht, kaufen sie wie die anderen.
- Alles NOT from data. Test: HB_BOTKP=5000 (sofort Panzer + Hubschrauber).
- Drohnen: in Arbeit (eigener Abschnitt folgt).

## Bots benutzen Drohnen (2026-10-22, Nutzer 21:32 "sie sollen auch drohnen nutzen")
- Neues Modul botdrones.rs. Bots mit einem Drohnen-Item in der Ausrüstung (Standard: Weste+Bodendrohne, Bodendrohne+Hellfire, Luftdrohne+PROX; Panzerabwehr- und Aufklärungsdrohne nur über HB_BOTDRONEKIND) kaufen die Drohne mit ihren KP und setzen sie 1,2 m vor sich ab (Luftdrohnen in Augenhöhe).
  - Der Bot bleibt geduckt an seinem Platz stehen und steuert die Drohne (botextras `Mode::Drone`: kein Infanterie-Verhalten, steigt in kein Fahrzeug). Er bleibt sichtbar und kann erschossen werden.
  - Ende: Batterie leer (Drohne verschwindet), Drohne zerstört, oder der Bot stirbt (dann explodiert die Drohne, NOT from data).
  - Kill-Liste: "Name: <Drohnenname>" beim Absetzen (Name aus CombatData::special_names), beim Abschuss "<Schütze> [Waffe] <Drohne> (<Besitzer>)". NOT from data. Für das Team des Spielers spielt der Kauf-Stinger (SC_Purchased_Drone).
  - Bot-Log: Kauf, Ziele, Raketen auf Fahrzeuge, Zerstörung (mit Kills während die Drohne draußen war).
- Aus den Daten (wie die Spieler-Drohne, drone.rs / main.rs load_drone): Meshes, Skelett, AnimTree-Rig (Ketten, Räder, Turm, Rotoren, Antenne), Spiegel-Anbauteile, Kollisionszylinder, Geschwindigkeit (GroundSpeed / AirSpeed), Batterie (VehicleFuelInfo: MaxFuel, fahren / stehen / je Schuss), Leben (DRONE_HEALTH der Spieler-Drohne), MG (Seats[0].GunClass: FireInterval, FireSpread, WeaponRange, FullDamageRange, Überhitzen, Schadenstyp, Schuss-Sound), Raketen (CombatData::drone_rockets: Magazin, Salve, Abstand, Nachladen, Streuung, Rakete, Lock-on) an den Seat0FireMode-Sockets, Motor-Sound (VehicleAudio SCEngineIdle), Markier-Reichweite (DroneLockOnDistanceSQ) und DroneMarkDuration der Aufklärungsdrohne, Kosten (SpecialAbilityCosts), Belohnung PSR_DestroyDrone 130 (GCBattlePoints Rewards), Zerstörungs-Effekt der Spieler-Drohne (FX_C4_EXP).
- Verhalten:
  - Kauf: Denkt jede Sekunde; nur zu Fuß, Killstreak-Abklingzeit (BotKit ks_cd) abgelaufen, genug KP. Gegner innerhalb 60 m oder offenes Ziel der aktiven Linie innerhalb 80 m, dann 50 % pro Prüfung. Danach 30 s Abklingzeit. Höchstens 2 Bot-Drohnen pro Team, 3 Exemplare pro Drohnenart. NOT from data.
  - Bodendrohnen fahren über das Laufgitter (nav.rs, wie die Bot-Fahrzeuge) zum nächsten offenen Ziel bzw. Gegner, Ketten zeigen in Fahrtrichtung, der Körper dreht zum Ziel. Kein Fortschritt für 5 s: neu planen und 1 s seitlich ausweichen. NOT from data.
  - Luftdrohnen fliegen dieselben Wege 6 m über dem Boden, steigen vor Hindernissen bis zu 15 m höher; Beschleunigung und Neigung wie die Spieler-Luftdrohne (FLY_ACCEL / FLY_TILT). NOT from data.
  - Ziele (alle 0,3 s): nächster Gegner in Sicht und Reichweite (Soldaten, Spieler, Spieler-Drohne, gegnerische Bot-Drohnen; Raketendrohnen bevorzugen Fahrzeuge, Soldaten erst ab 8 m). Mit Ziel in 70 % der Reichweite bleibt die Drohne stehen bzw. schwebt. NOT from data: Reichweite Raketen 60 m, Zielwahl.
  - Bodendrohne (Angriff): MG wie die Bot-Fahrzeugschützen: Reaktion 0,6 s / Skill, Feuerstöße 1,6 s an / 0,7 s aus, feuert innerhalb 4°, zusätzliche Streuung 1° / Skill (NOT from data). Schaden über das Schadensraster wie bei der Spieler-Drohne.
  - Panzerabwehr / Luft-Angriff: Raketen der Drohnenwaffe, gegen Fahrzeuge der Salven-Modus mit Vorhalt; bei Lock-on suchen die Raketen das Ziel (Seek wie bei den Werfern). Abschuss außerhalb der eigenen Box.
  - Aufklärung (Parrot): schießt nicht, markiert Gegner in Sicht innerhalb der Markier-Reichweite. Für das Team des Spielers mit den roten Rauten auf HUD/Minimap (ReconMarks). Für die Bots läuft solange die Team-UAV (botextras team_uav). NOT from data: die Weitergabe an das Team.
  - Motor: SfxQueue::looping_layers pro Drohne an ihrer Position, "Gas" beim Fahren; Lautstärke 0,6 im Stand, 1,0 in Fahrt (NOT from data).
- Treffer:
  - Jede Bot-Drohne hat eine Trefferbox (Mesh-Grenzen um den Mesh-Ursprung, NOT from data: Box statt Kollision) in VehicleVolumes und vehicles::SHOT_VOLUMES. Damit gilt: Raketen schlagen ein, Explosionen treffen sie, der PROX-Lock-on des Spielers erfasst sie, Kugeln bleiben an ihr hängen.
  - Kugeln machen Schaden (botdrones::bullet_hit): vom Spieler (combat.rs, mit Trefferanzeige), von Bots (bots.rs), von der Spieler-Drohne (drone.rs) und von Bot-Drohnen untereinander.
  - Neues combat::Target::BotDrone(Entity); Infanterie-Bots nehmen gegnerische Bot-Drohnen als Ziel (bots.rs).
  - Kein Eigenbeschuss. Bei 0 Leben: FX_C4_EXP wie die Spieler-Drohne plus Sound A2_Destruction_C.SC_DroneSmallVehicleAnnihilation (NOT from data: nach dem Namen gewählt). Der Zerstörer bekommt PSR_DestroyDrone 130 KP (Spieler: Banner).
- Nicht umgesetzt: Bot-Werfer (PZF3/PROX) und Fahrzeugschützen zielen nicht auf Drohnen (Fahrzeug-MG-Kugeln bleiben an der Box hängen, ohne Schaden), Drohnen auf der Minimap, EMP gegen Drohnen, Bots holen die Drohne nicht freiwillig zurück, der Bot hält kein Steuergerät in der Hand.
- Ladezeit: je Drohnenart 3 zusätzliche Mesh-Kopien (botdrones::INSTANCES).
- Test: HB_BOTDRONE=1 (Bots mit Drohnen-Item setzen sofort ab, ohne Abklingzeit/Lageprüfung, gratis wenn die KP fehlen), HB_BOTDRONEKIND=<0..3> (alle Drohnen-Bots nehmen diese Art: 0 Boden-Angriff, 1 Panzerabwehr, 2 Luft-Angriff, 3 Aufklärung), HB_BOTDRONEDBG=1 (Zustand jeder Bot-Drohne einmal pro Sekunde im Log), HB_BOTKP=5000, HB_BOTLOG=1.
- Container (Crossroads FL, HB_BOTDRONE=1 HB_BOTKP=5000, ca. 50 s Spielzeit): 3 Bodendrohnen + 1 Luftdrohne abgesetzt, 3 Kopien je Art geladen. Die Bodendrohnen fuhren zu den Gegnern und nahmen sie ins Ziel; ein Bediener bekam einen Kill. Kein Absturz. NICHT geprüft (Container zu langsam, zweiter Lauf wegen Speicher abgebrochen): Raketendrohnen, Aufklärung, Zerstörung, Darstellung. Bitte am PC testen.

## Bots: kein Wissen durch Wände, kein Dauer-Verfolgen (2026-10-22, Nutzer 22:02 / 22:04)
- Nutzer: "bots can often see me through walls or once they saw me they always know where I'm at" / "they can see me again even when I'm not in their view".
- Verfolgen ("hunt", wenn das Team alle Ziele hält, und in TDM immer): Bisher ging es zur LIVE-Position des nächsten Gegners, neu geplant alle 2–3 s. Das war Allwissen.
  - Jetzt nur zu Positionen, die das Team kennt (`bots.rs` TeamIntel). Dazu zählt, was ein Bot des Teams gesehen oder gehört hat (Schüsse im Hörradius), jeweils 8 s lang.
  - Während der UAV-Abtastung des Teams sind alle Gegner bekannt.
  - Ohne Wissen: Streifen wie vorher (TDM: ferne Spawns).
- Sichtprüfung auf den Spieler: Sie prüft jetzt Kopf und Brust auf seiner echten Höhe (geduckt: Pawn::eye). Vorher galten die Stehhöhe +20 / +45 uu, so sahen Bots einen geduckten Spieler über niedrige Deckung.
- Rundumsicht ohne Blickrichtung nur noch bis 2,5 m (vorher 5 m). Sonst gilt das Sichtfeld ±60° (nach einem Treffer 2 s rundum wie bisher).
- Sichtweite nachts kürzer: 45 % bei Nacht, gleitend bis 100 % am Tag (weather::daylight).
- Alles NOT from data.

## Atomschlag eines Bots: Fulton-Abholung sichtbar (2026-10-22, Nutzer 22:08 "when the bots are using an atom bomb I couldn't see him getting picked up by the plane with the full animation")
- Bisher lief im Bot-Modus (Endspiel-Atomschlag, von einem Bot des Gewinnerteams gerufen) nur Countdown, Sirene, Sprengkopf, Detonation und Druckwelle. Ballon und Flugzeug fehlten.
- Jetzt (`nuke.rs`): Der rufende Bot wird wie der Spieler abgeholt, mit denselben Zeiten, derselben Geometrie und denselben Sounds wie in der Spieler-Version:
  - DEPLOY: Der Ballon steigt über dem Bot auf, unter einem Dach durch den nächsten freien Himmel.
  - Das Flugzeug kommt über Ground Zero herein und schnappt bei CATCH die Leine.
  - Der Bot wird hochgerissen und hängt unter und hinter dem steigenden Flugzeug, bis es weg ist.
  - Danach Sprengkopf, Detonation, White-out und Fernsicht wie bisher. Die Fernsicht kommt weiter von der Spielerseite (`view_head`).
- Rufer: ein lebender Bot des Teams zu Fuß. Bots, die botextras.rs steuert (Fahrzeug, Werfer, Drohne), werden nur genommen, wenn kein anderer lebt; dann gibt es keine Abholung (wie bisher).
- Der Bot am Seil (`nuke::held` / `nuke::towed_pose`):
  - Ab dem Ruf ruht seine KI (bots.rs bot_ai: wie die von botextras gesteuerten Bots).
  - Er ist kein Ziel: Bots nehmen ihn wie einen Toten (bots.rs `others`).
  - Er nimmt keinen Schaden (combat.rs apply_damage), auch nicht von der Druckwelle (nuke.rs überspringt ihn).
  - Er setzt keinen Werfer ein (botextras.rs) und steigt in kein Fahrzeug (botvehicles.rs).
  - Bis CATCH steht er still (stand_idle).
- Körper am Seil: render_bots (bots.rs) zeichnet den eigenen Skin-Körper des Bots mit der Pose aus nuke.rs. Das Geschirr (Seilende) sitzt 1,35 m über den Füßen und 0,18 m vor der Brust.
  - Beim Ruck: Sequenz jump_up, danach jump_dn (Spiel-Sequenzen aus AS_CHC_PL_3P, Daten).
  - Der Körper dreht sich in 0,5 s von aufrecht (eigene Blickrichtung) auf die Seilrichtung und schaut in Flugrichtung.
  - Soldier.feet folgt dem Körper. Ist das Flugzeug weg, bleibt die Pose stehen.
  - Der Körper wird jedes Bild neu geskinnt (kein Skinning-Budget). render_bots läuft nach dem nuke-Update (`NukeSet`).
  - Pose und Seil werden im selben Bild berechnet; das Debug-Log hat das bestätigt. In Container-Aufnahmen schien der Körper bei voller Schleppgeschwindigkeit trotzdem bis zu ~2 Bilder hinter dem Seilende zu hängen. Die Testkamera (HB_BOTCAM) übernimmt hier aber die Blickrichtung des Spielers, deshalb ist das nicht sicher. Stellschraube: `HB_TOWLAG=<Bilder>` lässt die gezeichnete Pose so viele Bilder vorauslaufen (Standard 0). Falls am PC eine Lücke zwischen Seilende und Körper sichtbar ist, mit 1 oder 2 testen. Soldier.feet bleibt die aktuelle Lage.
- Seil im Bot-Modus: mindestens 0,8 mm Radius pro Meter Abstand zum Auge des Spielers (bei 150 m etwa 12 cm). So bleibt es von weitem 1–2 Pixel breit; die echten 2,2 cm wären unsichtbar. NOT from data.
- Blick des Spielers: Von DEPLOY bis CATCH + 2,5 s dreht sich seine Blickrichtung (Pawn yaw/pitch, also seine eigene Kamera samt Waffe) weich auf das Geschehen: erst den Ballon, dann das anfliegende Flugzeug, nach dem Schnappen auf den Bot am Seil mit dem Flugzeug darüber. Danach hat er die Steuerung wieder, ab der Richtung, in die er gerade schaut. Seine Position bleibt unverändert. Tot, im Spawn-Menü, im Fahrzeug oder mit Drohne: kein Eingriff. Verdeckte Sicht wird hingenommen.
- Sounds im Bot-Modus: Es sind dieselben Wellen, nur leiser je nach Entfernung des Spielers. Halbe Lautstärke bei 8 m (Ballon-Start), 120 m (Flugzeug-Vorbeiflug zum Schnapp-Punkt, Motor zum Flugzeug) und 25 m (Ruck). Wind und Mittelfinger gibt es nur in der Spieler-Version.
- NOT from data (das Spiel hat keinen Atomschlag): die ganze Abholung des Bots, Geschirr-Lage, Körperdrehung am Seil, Vorlauf der gezeichneten Pose (HB_TOWLAG), Seildicke nach Entfernung, die Wahl von jump_up/jump_dn als Hängepose, die Blickführung (Dauer 2,5 s nach CATCH, Drehrate 1,5–4/s), die Entfernungs-Lautstärke, die Unverwundbarkeit und das Ruhen der KI des Rufers.
- Die Spieler-Version ist unverändert.
- Dateien: nuke.rs; kleine Abfragen in bots.rs (KI ruht, kein Ziel, Körper-Pose, Systemreihenfolge), combat.rs (kein Schaden), botextras.rs (kein Werfer), botvehicles.rs (kein Einsteigen).
- Test: Spieler im Team USA (Standard), `HB_ENDNUKE=20,1`. Nach 20 s Spiel bekommt KVA den Atomschlag, ein KVA-Bot ruft ihn (Crawl "<Name>: ATOMSCHLAG ANGEFORDERT"). Im Log steht "nuke: bot ... is picked up at ...".

## Tod durch den Atomschlag eines Bots: Ragdoll statt HUD (2026-10-23 00:14)
- Nutzer: "when I die from the nuke my crosshair and such is still seen? can you make it so I just ragdoll to the ground" / "only when I die by the nuke, when I'm the one calling the nuke I shouldn't die".
- Bisher: Die Nuke setzte bei einem Tod durch die Druckwelle jedes Bild `dead_for` und `hud.death` zurück (kein Respawn bis zum Matchende). Damit blieb das ganze HUD sichtbar, auch Fadenkreuz, Munition und Minimap, und die Ansicht blieb im Kopf.
- Jetzt (nur Bot-Atomschlag, nach der Detonation, Spieler tot):
  - `nuke.rs` hide_hud(): `hud.rs` blendet das ganze HUD aus.
  - Die Kill-Cam zeigt wie bei jedem Tod den eigenen Körper als Ragdoll (`bots.rs` render_bots), weggestoßen von der Druckwelle.
  - Weißblende und Fernblick der Nuke übernehmen danach die Kamera wie bisher (`nuke.rs` camera_owned; die Kill-Cam setzt die Kamera dann nicht).
- Eigener Atomschlag: unverändert, der Spieler wird abgeholt und stirbt nicht.
- NOT from data.

## Ladefilm die ersten Sekunden schwarz (2026-10-23 00:29)
- Die Render-Pipeline des Film-Materials (movie.rs MovieMat) wurde erst angefordert, als der Ladefilm erschien. Zu der Zeit stellt das Match-Vorladen (warmup.rs) rund 400 Spiel-Pipelines in die Warteschlange; der Film blieb schwarz, während sein Ton schon lief.
- Jetzt wird beim Programmstart ein unsichtbar kleines Film-Quad gesetzt, damit die Pipeline vorher gebaut ist. NOT from data.
- Log: "movie: first frame on screen X s after the sound started".

## Farbiges Bild-Flackern: Leuchtspur-Sprite an der Kamera (2026-10-23 00:30, Farm)
- Das Video vom 20:48 genauer: Die Störbilder (schwarz / weiß / rot) kommen im Takt der Schüsse (alle 3–6 Videobilder, 60 fps). Die Augenanpassung ist danach unverändert, also war nur das Bild einzelner Frames betroffen.
- Ursache-Kandidat: Das Leuchtspur-Partikel selbst direkt an der Kamera. Bot-Leuchtspuren enden am Kopf des Spielers, wenn er getroffen wird; die eigenen starten an der 1P-Mündung. Das Sprite füllt dann für ein Bild den Bildschirm. Bisher wurde nur das Licht (dynlights) an der Kamera ausgeblendet.
- `combat.rs` fire_tracer: Kein Teil einer Leuchtspur liegt näher als 2 m an der Kamera (TRACER_CAM_CLEAR).
  - Eine Spur, die auf die Kamera zufliegt, endet 3 m davor.
  - Eine Spur, die an der Kamera beginnt, startet erst beim Verlassen der 2-m-Kugel.
  - Liegt eine Spur ganz darin, entfällt sie.
- Kameraposition: dynlights.rs setzt sie jedes Frame (combat::set_camera_ue).
- NOT from data.

## Flacker-Suche: Diagnose-Schalter und Mittelwert-Aufnahme (2026-10-23 10:00, Nutzer: "the glitching screen is still there but now only after launching a nuke")
- Env-Schalter zum Eingrenzen (main.rs `diag`): HB_NOFOGPASS (weather.rs FogNode), HB_NOSCREENRAIN, HB_NOSCENECOPY, HB_NODYNLIGHTS. Ohne Schalter unverändert.
- record.rs: jedes aufgenommene Bild schreibt seinen Farb-Mittelwert (mittlere 60 %) nach means.csv; HB_RECORD_NOPNG=1 spart die PNGs. fps 0 = Echtzeit (kein fester Zeitschritt).
- player.rs HB_TEST_TURN=<Grad/s>: dauerndes Drehen für Tests.
- Leave-Match-Reset in nuke.rs läuft jetzt auch, wenn nur noch Post-Pass / Wolke aktiv sind (nk.running || fxp.on || cloud.on): NukeFx, Wolke und Krater tragen nicht ins nächste Match.
- Alles Testwerkzeug, NOT from data.

## Release-Paket (2026-10-23, Nutzer: "make a exe now with all the assets needed to start")
- tools\make_release.cmd (Doppelklick) -> tools\make_release.ps1: cargo build --release, dann dist\HomeBots\ mit HomeBots.exe, assets\ (eigene Dateien: Atomschlag-Modelle/Animationen/Töne, PA-ogg), LIESMICH.txt, spielpfad.txt; dazu dist\HomeBots.zip. Log: logs\release_build.txt.
- Spieldaten werden NICHT mitgeliefert: die exe liest sie aus der Homefront-Installation. Pfad: HOMEBOTS_GAME, sonst erste Nicht-#-Zeile von spielpfad.txt, sonst der Steam-Standardpfad.
- main.rs: fehlt ./assets, wird das Arbeitsverzeichnis auf den exe-Ordner gesetzt (Start per Doppelklick / Verknüpfung). Release-Build ohne Konsolenfenster (windows_subsystem).

## Release-Paket englisch (2026-10-23 11:26, Nutzer: "mach die liesmich und spielpfad datei englisch"; Sprache bleibt die Steam-Sprache des Spiels)
- dist enthält README.txt (statt LIESMICH.txt) und gamepath.txt (statt spielpfad.txt); main.rs liest gamepath.txt, spielpfad.txt weiter als Ausweichname.
- Kill-Anzeige der Bot-Luftschläge (botextras.rs): Name aus den Spieltexten der eingestellten Sprache (CombatData::special_names), sonst englischer Ersatz; botdrones.rs-Ersatznamen englisch. Ersatznamen NOT from data.
