"""Milan Pro brand: renders the 3D icon objects in Blender (background mode only, no window).

    blender -b --factory-startup --python tools/brand/render.py -- <object> <out.png> [size] [variant]

<object> is one of
    reel     the Milan Pro mark: a blue anodised film reel, 3/4 view, with an orange play wedge and a film tail
    clapper  exploration: a clapperboard with a chrome hinge
    orb      exploration: a glossy lens orb with a filmstrip swoosh
[variant] "small" drops the film tail and thickens the parts (the 32-48 px source).

The render is a transparent PNG lit the Windows XP way: a big soft key light from the upper left, a weak fill from
the right, a sky-over-horizon environment for the metal, the Standard view transform (saturated colours, no
filmic roll-off). Outline and drop shadow are added afterwards by tools/brand/compose.py, as XP icons did in
Photoshop.
"""
import bpy
import bmesh
import math
import os
import sys
from mathutils import Vector, Matrix

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
WHAT = argv[0] if argv else "reel"
OUT = os.path.abspath(argv[1] if len(argv) > 1 else "render.png")
SIZE = int(argv[2]) if len(argv) > 2 else 1024
VARIANT = argv[3] if len(argv) > 3 else "full"
SMALL = VARIANT == "small"


def lin(hexstr):
    """sRGB hex to linear RGBA."""
    h = hexstr.lstrip("#")
    c = [int(h[i:i + 2], 16) / 255.0 for i in (0, 2, 4)]
    c = [x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4 for x in c]
    return (*c, 1.0)


# ---------------------------------------------------------------------------------------------------------- scene
bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
sc.render.engine = "CYCLES"
sc.cycles.device = "CPU"
sc.cycles.samples = 128
sc.cycles.use_adaptive_sampling = True
sc.cycles.use_denoising = True
sc.cycles.max_bounces = 8
sc.cycles.transparent_max_bounces = 16
sc.render.film_transparent = True
sc.render.resolution_x = sc.render.resolution_y = SIZE
sc.render.resolution_percentage = 100
sc.render.filter_size = 1.2
sc.view_settings.view_transform = "Standard"
sc.render.image_settings.file_format = "PNG"
sc.render.image_settings.color_mode = "RGBA"
sc.render.image_settings.color_depth = "8"
sc.render.filepath = OUT


def node_tree(idblock):
    try:
        idblock.use_nodes = True
    except Exception:
        pass
    return idblock.node_tree


# World: light-blue sky over a white horizon over a dark slate floor, so metal shows the classic chrome bands.
world = bpy.data.worlds.new("Sky")
sc.world = world
wt = node_tree(world)
wn, wl = wt.nodes, wt.links
bg = next(n for n in wn if n.type == "BACKGROUND")
tc = wn.new("ShaderNodeTexCoord")
sep = wn.new("ShaderNodeSeparateXYZ")
mr = wn.new("ShaderNodeMapRange")
mr.inputs[1].default_value, mr.inputs[2].default_value = -1.0, 1.0
ramp = wn.new("ShaderNodeValToRGB")
els = ramp.color_ramp.elements
els[0].position, els[0].color = 0.0, lin("#2a3140")
els[1].position, els[1].color = 1.0, lin("#7fa6e0")
for pos, col in ((0.40, "#4a5466"), (0.445, "#ffffff"), (0.55, "#e4edfa"), (0.75, "#a9c4ec")):
    e = els.new(pos)
    e.color = lin(col)
wl.new(tc.outputs["Generated"], sep.inputs[0])
wl.new(sep.outputs["Z"], mr.inputs[0])
wl.new(mr.outputs[0], ramp.inputs[0])
wl.new(ramp.outputs[0], bg.inputs[0])
bg.inputs[1].default_value = 0.9


def light(name, kind, loc, energy, size=1.0, color="#ffffff"):
    ld = bpy.data.lights.new(name, kind)
    ld.energy = energy
    ld.color = lin(color)[:3]
    if kind == "AREA":
        ld.shape = "DISK"
        ld.size = size
    ob = bpy.data.objects.new(name, ld)
    sc.collection.objects.link(ob)
    ob.location = loc
    ob.rotation_euler = (Vector((0, 0, 0)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
    return ob


light("Key", "AREA", (-5.0, -6.0, 7.0), 2600, 5.0)          # upper left, in front
light("Fill", "AREA", (6.0, -4.0, 0.5), 450, 4.0, "#e8f0ff")  # right, low, cool
light("Rim", "AREA", (3.0, 6.0, 5.0), 900, 3.0)               # behind, top


# ---------------------------------------------------------------------------------------------------------- materials
def principled(m):
    return next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")


def setin(node, names, value):
    for nm in names if isinstance(names, (list, tuple)) else (names,):
        if nm in node.inputs:
            node.inputs[nm].default_value = value
            return


def material(name, color, metallic=0.0, rough=0.3, coat=0.0, aniso=0.0, spec=0.5):
    m = bpy.data.materials.new(name)
    node_tree(m)
    p = principled(m)
    setin(p, "Base Color", lin(color) if isinstance(color, str) else color)
    setin(p, "Metallic", metallic)
    setin(p, "Roughness", rough)
    setin(p, ("Coat Weight", "Coat"), coat)
    setin(p, "Coat Roughness", 0.03)
    setin(p, "Anisotropic", aniso)
    setin(p, ("Specular IOR Level", "Specular"), spec)
    return m


ANODISED = material("Anodised blue", "#2a6ae8", metallic=0.55, rough=0.28, coat=1.0)
ALU = material("Brushed aluminium", "#c9d0d9", metallic=1.0, rough=0.22, aniso=0.6)
CHROME = material("Chrome", "#f2f4f7", metallic=1.0, rough=0.06)
SILVER = material("Satin silver", "#dfe4ea", metallic=0.85, rough=0.32)
FILMPACK = material("Wound film", "#2b2118", metallic=0.0, rough=0.25, spec=0.7)
ORANGE = material("Play orange", "#ff8a14", metallic=0.0, rough=0.2, coat=1.0)
INK = material("Slate black", "#16181c", rough=0.3, coat=0.6)
WHITE = material("Paint white", "#f4f6f8", rough=0.35)
ORB = material("Orb blue", "#0f3fae", metallic=0.0, rough=0.08, coat=1.0, spec=0.8)
GLASS = material("Lens glass", "#08142e", metallic=0.2, rough=0.03, coat=1.0, spec=1.0)


def film_material():
    """Film stock: a dark base with sky-blue frames and see-through sprocket holes, mapped on the ribbon's UVs
    (U across the strip, V along it, one frame per V unit)."""
    m = bpy.data.materials.new("Film")
    nt = node_tree(m)
    n, l = nt.nodes, nt.links
    p = principled(m)
    setin(p, "Roughness", 0.18)
    setin(p, ("Coat Weight", "Coat"), 0.6)
    uv = n.new("ShaderNodeTexCoord")
    sp = n.new("ShaderNodeSeparateXYZ")
    l.new(uv.outputs["UV"], sp.inputs[0])

    def math(op, a, b=None):
        mn = n.new("ShaderNodeMath")
        mn.operation = op
        for i, v in enumerate((a, b)):
            if v is None:
                continue
            if isinstance(v, (int, float)):
                mn.inputs[i].default_value = v
            else:
                l.new(v, mn.inputs[i])
        return mn.outputs[0]

    u, v = sp.outputs["X"], sp.outputs["Y"]
    # Sprocket holes: |u - 0.5| > 0.36 and fract(v * 4) in [0.25, 0.75].
    edge = math("GREATER_THAN", math("ABSOLUTE", math("SUBTRACT", u, 0.5)), 0.355)
    edge_in = math("LESS_THAN", math("ABSOLUTE", math("SUBTRACT", u, 0.5)), 0.44)
    fv = math("FRACT", math("MULTIPLY", v, 4.0))
    hole_v = math("LESS_THAN", math("ABSOLUTE", math("SUBTRACT", fv, 0.5)), 0.22)
    hole = math("MULTIPLY", math("MULTIPLY", edge, edge_in), hole_v)
    alpha = math("SUBTRACT", 1.0, hole)
    l.new(alpha, p.inputs["Alpha"])
    # Frames: |u - 0.5| < 0.29 and fract(v) in [0.06, 0.94].
    fr = math("FRACT", v)
    frame_v = math("LESS_THAN", math("ABSOLUTE", math("SUBTRACT", fr, 0.5)), 0.42)
    frame_u = math("LESS_THAN", math("ABSOLUTE", math("SUBTRACT", u, 0.5)), 0.29)
    frame = math("MULTIPLY", frame_v, frame_u)
    sky = n.new("ShaderNodeValToRGB")
    se = sky.color_ramp.elements
    se[0].position, se[0].color = 0.0, lin("#3d7be0")
    se[1].position, se[1].color = 1.0, lin("#e9f2ff")
    l.new(fr, sky.inputs[0])
    mix = n.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    l.new(frame, mix.inputs["Factor"])
    mix.inputs[6].default_value = lin("#24201c")
    l.new(sky.outputs[0], mix.inputs[7])
    l.new(mix.outputs[2], p.inputs["Base Color"])
    return m


FILM = film_material()


def stripes_material():
    """Clapper stick: black and white diagonal bands in object space."""
    m = bpy.data.materials.new("Stripes")
    nt = node_tree(m)
    n, l = nt.nodes, nt.links
    p = principled(m)
    setin(p, "Roughness", 0.25)
    setin(p, ("Coat Weight", "Coat"), 0.8)
    tc = n.new("ShaderNodeTexCoord")
    sp = n.new("ShaderNodeSeparateXYZ")
    l.new(tc.outputs["Object"], sp.inputs[0])
    add = n.new("ShaderNodeMath")
    add.operation = "ADD"
    l.new(sp.outputs["X"], add.inputs[0])
    l.new(sp.outputs["Z"], add.inputs[1])
    mul = n.new("ShaderNodeMath")
    mul.operation = "MULTIPLY"
    l.new(add.outputs[0], mul.inputs[0])
    mul.inputs[1].default_value = 1.9
    fr = n.new("ShaderNodeMath")
    fr.operation = "FRACT"
    l.new(mul.outputs[0], fr.inputs[0])
    gt = n.new("ShaderNodeMath")
    gt.operation = "GREATER_THAN"
    l.new(fr.outputs[0], gt.inputs[0])
    gt.inputs[1].default_value = 0.5
    mix = n.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    l.new(gt.outputs[0], mix.inputs["Factor"])
    mix.inputs[6].default_value = lin("#15171b")
    mix.inputs[7].default_value = lin("#f5f6f8")
    l.new(mix.outputs[2], p.inputs["Base Color"])
    return m


# ---------------------------------------------------------------------------------------------------------- geometry
ROOT = bpy.data.objects.new("Root", None)
sc.collection.objects.link(ROOT)


def mesh_object(name, bm, mat, parent=ROOT, smooth_sides=True):
    me = bpy.data.meshes.new(name)
    if smooth_sides:
        for f in bm.faces:
            f.smooth = True
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    sc.collection.objects.link(ob)
    ob.parent = parent
    ob.data.materials.append(mat)
    return ob


def cylinder(name, r, depth, mat, segs=96, y=0.0, parent=ROOT):
    """Axis along Y (faces toward -Y, the camera side), centred at y. Caps flat, sides smooth."""
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, cap_tris=False, segments=segs, radius1=r, radius2=r, depth=depth)
    bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=Matrix.Rotation(math.radians(90), 3, "X"))
    bmesh.ops.translate(bm, verts=bm.verts, vec=(0, y, 0))
    for f in bm.faces:
        f.smooth = abs(f.normal.y) < 0.5
    return mesh_object(name, bm, mat, parent, smooth_sides=False)


def box(name, sx, sy, sz, mat, loc=(0, 0, 0), parent=ROOT):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=(sx, sy, sz), verts=bm.verts)
    bmesh.ops.translate(bm, verts=bm.verts, vec=loc)
    return mesh_object(name, bm, mat, parent, smooth_sides=False)


def bevel(ob, width, segs=3):
    md = ob.modifiers.new("Bevel", "BEVEL")
    md.width = width
    md.segments = segs
    md.limit_method = "ANGLE"
    md.harden_normals = False
    return md


def cut(ob, cutter):
    cutter.hide_render = True
    cutter.display_type = "WIRE"
    md = ob.modifiers.new("Cut " + cutter.name, "BOOLEAN")
    md.operation = "DIFFERENCE"
    md.object = cutter
    md.solver = "EXACT"
    # Boolean before bevel.
    if len(ob.modifiers) > 1:
        bpy.context.view_layer.update()
        with bpy.context.temp_override(object=ob, active_object=ob):
            bpy.ops.object.modifier_move_to_index(modifier=md.name, index=0)
    return md


def ribbon(name, points, width, mat, parent=ROOT, up=Vector((0, 0, 1)), v_per_unit=1.0):
    """A filmstrip: a strip of quads along a polyline, `width` wide, UV u across, v along (1 per frame)."""
    bm = bmesh.new()
    uvl = bm.loops.layers.uv.new("UVMap")
    rows = []
    dist = 0.0
    for i, p in enumerate(points):
        p = Vector(p)
        t = (Vector(points[min(i + 1, len(points) - 1)]) - Vector(points[max(i - 1, 0)])).normalized()
        side = t.cross(up)
        if side.length < 1e-4:
            side = t.cross(Vector((1, 0, 0)))
        side.normalize()
        if i:
            dist += (p - Vector(points[i - 1])).length
        rows.append((bm.verts.new(p - side * width / 2), bm.verts.new(p + side * width / 2), dist / width * v_per_unit))
    for (a0, b0, v0), (a1, b1, v1) in zip(rows, rows[1:]):
        f = bm.faces.new((a0, b0, b1, a1))
        for loop, uv in zip(f.loops, ((0, v0), (1, v0), (1, v1), (0, v1))):
            loop[uvl].uv = uv
    ob = mesh_object(name, bm, mat, parent)
    sol = ob.modifiers.new("Thickness", "SOLIDIFY")
    sol.thickness = 0.02
    return ob


def build_reel():
    # Two anodised flanges with five round windows, a chrome hub, wound film between the flanges.
    gap = 0.16 if not SMALL else 0.12
    th = 0.07 if not SMALL else 0.09
    holes = []
    nh = 5
    for k in range(nh):
        a = math.radians(90 + k * 360 / nh)
        c = cylinder("hole%d" % k, 0.255 if not SMALL else 0.27, 2.0, ANODISED, 48)
        c.location = (math.cos(a) * 0.58, 0, math.sin(a) * 0.58)
        holes.append(c)
    for side in (-1, 1):
        fl = cylinder("flange%d" % side, 1.0, th, ANODISED, 128, y=side * (gap + th / 2))
        for h in holes:
            cut(fl, h)
        bevel(fl, 0.018, 3)
    film = cylinder("film", 0.80, gap * 2 + 0.002, FILMPACK, 128)
    hub = cylinder("hub", 0.2, gap * 2 + th * 2 + 0.08, SILVER, 64)
    bevel(hub, 0.02, 3)
    spindle = cylinder("spindle", 0.075, 2.0, CHROME, 32)
    cut(hub, spindle)
    for side in (-1, 1):
        ring = cylinder("hubring%d" % side, 0.34, 0.03, SILVER, 64, y=side * (gap + th + 0.012))
        bevel(ring, 0.008, 2)
    # Film tail leaving the bottom of the pack, sweeping out to the lower left toward the camera and turning up
    # (not at the small sizes).
    if not SMALL:
        pts = []
        for i in range(80):
            t = i / 79
            x = -0.05 - 1.15 * t
            y = -0.05 - 0.55 * t
            z = -0.80 - 0.32 * math.sin(t * math.pi) + 0.30 * t * t
            pts.append((x, y, z))
        ribbon("tail", pts, 0.34, FILM, up=Vector((0.25, -1.0, 0.15)))
    # Play wedge: an orange triangular slab in front, lower right, nearly facing the camera.
    s = 0.48 if not SMALL else 0.56
    bm = bmesh.new()
    vs = [bm.verts.new((s * math.cos(a), 0, s * math.sin(a))) for a in (0, 2 * math.pi / 3, 4 * math.pi / 3)]
    f = bm.faces.new(vs)
    ext = bmesh.ops.extrude_face_region(bm, geom=[f])
    bmesh.ops.translate(bm, verts=[v for v in ext["geom"] if isinstance(v, bmesh.types.BMVert)], vec=(0, 0.16, 0))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    wedge_root = bpy.data.objects.new("WedgeRoot", None)
    sc.collection.objects.link(wedge_root)
    wedge = mesh_object("wedge", bm, ORANGE, wedge_root, smooth_sides=False)
    bevel(wedge, 0.07, 4)
    wedge_root.location = (0.62, -0.62, -0.50) if not SMALL else (0.55, -0.6, -0.45)
    wedge_root.rotation_euler = (0, 0, math.radians(18))
    return dict(yaw=-32, pitch=12, cam_elev=16)


def build_clapper():
    body = box("slate", 2.0, 0.14, 1.32, INK, (0, 0, -0.30))
    bevel(body, 0.04, 3)
    # painted lines on the slate
    for z in (-0.12, -0.52):
        box("line", 1.84, 0.02, 0.035, WHITE, (0, -0.075, z))
    box("vline", 0.035, 0.02, 0.40, WHITE, (0.1, -0.075, -0.32))
    stripes = stripes_material()
    lower = box("stick_lower", 2.0, 0.15, 0.26, stripes, (0, 0, 0.50))
    bevel(lower, 0.02, 2)
    hinge_root = bpy.data.objects.new("HingeRoot", None)
    sc.collection.objects.link(hinge_root)
    hinge_root.parent = ROOT
    hinge_root.location = (-0.98, 0, 0.66)
    upper = box("stick_upper", 2.0, 0.15, 0.26, stripes, (1.0, 0, 0.14), parent=hinge_root)
    bevel(upper, 0.02, 2)
    hinge_root.rotation_euler = (0, math.radians(-20), 0)
    hinge = cylinder("hinge", 0.11, 0.30, CHROME, 48)
    hinge.location = (-0.98, 0, 0.66)
    bevel(hinge, 0.02, 2)
    blue = box("band", 2.0, 0.145, 0.22, ANODISED, (0, 0, -0.84))
    bevel(blue, 0.02, 2)
    return dict(yaw=-28, pitch=8, cam_elev=14)


def build_orb():
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=96, v_segments=48, radius=0.9)
    mesh_object("orb", bm, ORB)
    # chrome bezel and dark lens on the front
    bez = cylinder("bezel", 0.62, 0.20, CHROME, 96, y=-0.78)
    bevel(bez, 0.05, 4)
    lens = cylinder("lens", 0.50, 0.06, GLASS, 96, y=-0.89)
    bevel(lens, 0.03, 3)
    inner = cylinder("inner", 0.26, 0.02, CHROME, 64, y=-0.925)
    bevel(inner, 0.008, 2)
    # filmstrip swoosh orbiting, tilted
    pts = []
    for i in range(140):
        a = math.radians(-200 + 250 * i / 139)
        r = 1.22 + 0.06 * math.sin(a * 2)
        pts.append((r * math.cos(a), r * math.sin(a), 0.18 * math.sin(a + 0.6)))
    sw = ribbon("swoosh", pts, 0.36, FILM, up=Vector((0, 0, 1)))
    # the ribbon lies in the XY plane, so its width runs radially: stand it on edge facing out.
    sw.rotation_euler = (math.radians(70), math.radians(-14), 0)
    return dict(yaw=-22, pitch=6, cam_elev=12)


params = {"reel": build_reel, "clapper": build_clapper, "orb": build_orb}[WHAT]()

ROOT.rotation_euler = (math.radians(params["pitch"]), 0, math.radians(params["yaw"]))
for ob in list(sc.objects):
    if ob.name == "WedgeRoot":
        ob.parent = ROOT
bpy.context.view_layer.update()

# ---------------------------------------------------------------------------------------------------------- camera
cam_data = bpy.data.cameras.new("Cam")
cam_data.lens = 85
cam = bpy.data.objects.new("Cam", cam_data)
sc.collection.objects.link(cam)
sc.camera = cam
elev = math.radians(params["cam_elev"])
direction = Vector((0, -math.cos(elev), math.sin(elev)))


def corners():
    out = []
    dg = bpy.context.evaluated_depsgraph_get()
    for ob in sc.objects:
        if ob.type != "MESH" or ob.hide_render:
            continue
        ev = ob.evaluated_get(dg)
        me = ev.to_mesh()
        mw = ob.matrix_world
        out.extend(mw @ v.co for v in me.vertices)
        ev.to_mesh_clear()
    return out


from bpy_extras.object_utils import world_to_camera_view  # noqa: E402

pts = corners()
centre = sum(pts, Vector()) / len(pts)
dist = 10.0
for _ in range(6):
    cam.location = centre + direction * dist
    cam.rotation_euler = (-direction).to_track_quat("-Z", "Y").to_euler()
    cam_data.shift_x = cam_data.shift_y = 0.0
    bpy.context.view_layer.update()
    proj = [world_to_camera_view(sc, cam, p) for p in pts]
    xs, ys = [p.x for p in proj], [p.y for p in proj]
    ext = max(max(xs) - min(xs), max(ys) - min(ys))
    dist *= ext / 0.90
cam.location = centre + direction * dist
bpy.context.view_layer.update()
proj = [world_to_camera_view(sc, cam, p) for p in pts]
xs, ys = [p.x for p in proj], [p.y for p in proj]
cam_data.shift_x = ((max(xs) + min(xs)) / 2 - 0.5)
cam_data.shift_y = ((max(ys) + min(ys)) / 2 - 0.5)

bpy.ops.render.render(write_still=True)
print("rendered", WHAT, VARIANT, OUT)
