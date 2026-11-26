// Import from Milan Pro (VEGAS Pro 13 and earlier; use "Import from Milan Pro.cs" in VEGAS Pro 14 and later).
//
// Rebuilds a Milan Pro project in VEGAS from the <name>.milan.json that Milan Pro writes with
// File > Export > VEGAS Project (via script)... Run it with Tools > Scripting > Run Script..., or copy it into VEGAS's
// "Script Menu" folder to get a menu item. Milan Pro never writes a .veg: this script builds the project through
// VEGAS's own scripting API, so what you get is a real VEGAS project you then save.
//
// Rebuilt: project video and audio settings, tracks (name, mute, solo, volume, pan, composite level and mode,
// parent/child compositing, Track Motion keyframes, envelopes, track FX), events (media, stream, in point, rate,
// reversed events as reversed subclips, fades and curves, gain/opacity, mute, lock, loop, normalize, groups, takes,
// Pan/Crop keyframes, velocity envelopes, event FX with parameters and keyframes, transitions, generated media),
// video output FX, master bus FX by name, markers, regions and the time selection. Plug-ins Milan Pro mapped onto
// VEGAS's are set up with their parameters; anything VEGAS does not have is listed at the end.
//
// Keep this file and the VEGAS 13 copy the same apart from the "using" line.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Windows.Forms;
using Sony.Vegas;

public class EntryPoint
{
    Vegas vegas;
    Project proj;
    List<string> notes = new List<string>();
    Dictionary<long, TrackEventGroup> groups = new Dictionary<long, TrackEventGroup>();

    public void FromVegas(Vegas v)
    {
        vegas = v;
        try {
            OpenFileDialog dlg = new OpenFileDialog();
            dlg.Title = "Import from Milan Pro";
            dlg.Filter = "Milan Pro export (*.milan.json)|*.milan.json|JSON (*.json)|*.json";
            if (dlg.ShowDialog() != DialogResult.OK) {
                return;
            }
            string text = File.ReadAllText(dlg.FileName, Encoding.UTF8);
            Dictionary<string, object> root = Json.Parse(text) as Dictionary<string, object>;
            if (root == null || Str(root, "format") != "milan-vegas") {
                MessageBox.Show("This is not a Milan Pro export (format \"milan-vegas\").", "Import from Milan Pro");
                return;
            }
            if (vegas.Project.Tracks.Count > 0 || vegas.Project.MediaPool.Count > 0) {
                DialogResult r = MessageBox.Show("Import into a new project? (No adds the tracks to the open project.)", "Import from Milan Pro", MessageBoxButtons.YesNoCancel);
                if (r == DialogResult.Cancel) {
                    return;
                }
                if (r == DialogResult.Yes && !vegas.NewProject(true, false)) {
                    return;
                }
            }
            proj = vegas.Project;
            Dictionary<string, object> settings = Obj(root, "project");
            if (settings != null) {
                ApplySettings(settings);
            }
            foreach (object t in List(root, "tracks")) {
                Dictionary<string, object> track = t as Dictionary<string, object>;
                if (track != null) {
                    AddTrack(track);
                }
            }
            foreach (object m in List(root, "markers")) {
                Dictionary<string, object> mk = m as Dictionary<string, object>;
                if (mk != null) {
                    proj.Markers.Add(new Marker(Tc(Num(mk, "t", 0)), Str(mk, "name")));
                }
            }
            foreach (object m in List(root, "regions")) {
                Dictionary<string, object> rg = m as Dictionary<string, object>;
                if (rg != null) {
                    double t = Num(rg, "t", 0), len = Num(rg, "length", 0);
                    if (len < 0) {
                        t += len;
                        len = -len;
                    }
                    proj.Regions.Add(new Region(Tc(t), Tc(len), Str(rg, "name")));
                }
            }
            Dictionary<string, object> loop = Obj(root, "loop");
            if (loop != null && Num(loop, "length", 0) > 0) {
                vegas.SelectionStart = Tc(Num(loop, "t", 0));
                vegas.SelectionLength = Tc(Num(loop, "length", 0));
            }
            AddVideoFx(proj.VideoBus.Effects, List(root, "output_fx"), "Video output FX");
            AddAudioFx(proj.MasterBus.Effects, List(root, "master_fx"), "Master bus FX");
            foreach (object s in List(root, "unsupported")) {
                if (s is string) {
                    notes.Add("Milan Pro: " + (string)s);
                }
            }
            string done = "Imported " + Path.GetFileName(dlg.FileName) + ". Save the project with File > Save As.";
            if (notes.Count > 0) {
                StringBuilder b = new StringBuilder(done + "\n\nNot carried over (" + notes.Count + "):\n");
                int shown = 0;
                foreach (string n in notes) {
                    if (shown++ == 30) {
                        b.Append("...\n");
                        break;
                    }
                    b.Append("- ").Append(n).Append('\n');
                }
                done = b.ToString();
            }
            MessageBox.Show(done, "Import from Milan Pro");
        } catch (Exception e) {
            MessageBox.Show("Import failed: " + e.Message, "Import from Milan Pro");
        }
    }

    void ApplySettings(Dictionary<string, object> s)
    {
        int w = (int)Num(s, "w", 0), h = (int)Num(s, "h", 0);
        if (w > 0 && h > 0) {
            proj.Video.Width = w;
            proj.Video.Height = h;
        }
        double fps = Num(s, "fps", 0);
        if (fps > 0) {
            proj.Video.FrameRate = fps;
        }
        double par = Num(s, "pixel_aspect", 0);
        if (par > 0) {
            proj.Video.PixelAspectRatio = par;
        }
        string fo = Str(s, "field_order");
        if (fo == "None") {
            fo = "ProgressiveScan";
        }
        proj.Video.FieldOrder = (VideoFieldOrder)Enum(typeof(VideoFieldOrder), fo, proj.Video.FieldOrder);
        proj.Video.RenderQuality = (VideoRenderQuality)Enum(typeof(VideoRenderQuality), Str(s, "quality"), proj.Video.RenderQuality);
        proj.Video.MotionBlurType = (MotionBlurType)Enum(typeof(MotionBlurType), Str(s, "motion_blur"), proj.Video.MotionBlurType);
        proj.Video.DeinterlaceMethod = (VideoDeinterlaceMethod)Enum(typeof(VideoDeinterlaceMethod), Str(s, "deinterlace"), proj.Video.DeinterlaceMethod);
        proj.Video.PixelFormat = (PixelFormat)Enum(typeof(PixelFormat), Str(s, "pixel_format"), proj.Video.PixelFormat);
        uint rate = (uint)Num(s, "sample_rate", 0);
        if (rate > 0) {
            proj.Audio.SampleRate = rate;
        }
        uint bits = (uint)Num(s, "bit_depth", 0);
        if (bits == 16 || bits == 24 || bits == 32) {
            proj.Audio.BitDepth = bits;
        }
    }

    void AddTrack(Dictionary<string, object> t)
    {
        string kind = Str(t, "kind");
        string name = Str(t, "name");
        if (kind == "video") {
            VideoTrack vt = proj.AddVideoTrack();
            vt.Name = name;
            vt.Mute = Bool(t, "mute", false);
            vt.Solo = Bool(t, "solo", false);
            vt.CompositeLevel = (float)Num(t, "level", 1);
            CompositeMode mode = (CompositeMode)Enum(typeof(CompositeMode), Str(t, "mode"), CompositeMode.SrcAlpha);
            if (mode == CompositeMode.Custom || mode == CompositeMode.Invalid) {
                mode = CompositeMode.SrcAlpha;
            }
            vt.SetCompositeMode(mode, false);
            if (Bool(t, "child", false)) {
                vt.CompositeNestingLevel = 1;
            }
            if (Bool(t, "parent", false)) {
                CompositeMode pm = (CompositeMode)Enum(typeof(CompositeMode), Str(t, "parent_mode"), CompositeMode.SrcAlpha);
                if (pm != CompositeMode.Custom && pm != CompositeMode.Invalid) {
                    vt.SetParentCompositeMode(pm, false);
                }
                SetMotion(vt.ParentTrackMotion, List(t, "parent_motion"));
            }
            SetMotion(vt.TrackMotion, List(t, "motion"));
            vt.TrackMotion.ShadowEnabled = Bool(t, "shadow", false);
            vt.TrackMotion.GlowEnabled = Bool(t, "glow", false);
            AddEnvelopes(vt.Envelopes, List(t, "envelopes"), vt, name);
            AddVideoFx(vt.Effects, List(t, "fx"), name + ": track FX");
            foreach (object e in List(t, "events")) {
                Dictionary<string, object> ev = e as Dictionary<string, object>;
                if (ev != null) {
                    AddVideoEvent(vt, ev, name);
                }
            }
        } else if (kind == "audio") {
            AudioTrack at = proj.AddAudioTrack();
            at.Name = name;
            at.Mute = Bool(t, "mute", false);
            at.Solo = Bool(t, "solo", false);
            at.Volume = (float)Num(t, "volume", 1);
            at.PanX = (float)Num(t, "pan", 0);
            AddEnvelopes(at.Envelopes, List(t, "envelopes"), at, name);
            // Milan's chain replaces the default track FX chain only when the export names any.
            List<object> fx = List(t, "fx");
            if (fx.Count > 0) {
                at.Effects.Clear();
                AddAudioFx(at.Effects, fx, name + ": track FX");
            }
            foreach (object e in List(t, "events")) {
                Dictionary<string, object> ev = e as Dictionary<string, object>;
                if (ev != null) {
                    AddAudioEvent(at, ev, name);
                }
            }
        } else {
            notes.Add("A track of kind \"" + kind + "\" was skipped");
        }
    }

    // ---- Events ----

    // The media stream for an event: the file (added to the pool), a reversed subclip of it, or generated media.
    MediaStream StreamFor(Dictionary<string, object> ev, bool video, string at, out double offset)
    {
        offset = Num(ev, "offset", 0);
        Dictionary<string, object> gen = Obj(ev, "generator");
        if (gen != null && video) {
            PlugInNode plug = FindPlugIn(vegas.Generators, gen);
            if (plug == null) {
                notes.Add(at + ": generator " + FxLabel(gen) + " is not installed; the event was skipped");
                return null;
            }
            Media m = new Media(plug);
            m.Length = Tc(Num(ev, "length", 1));
            if (m.Generator != null) {
                SetParams(m.Generator.OFXEffect, List(gen, "params"));
            }
            offset = 0;
            return m.GetVideoStreamByIndex(0);
        }
        string path = Str(ev, "path");
        if (path.Length == 0) {
            notes.Add(at + ": an event without media was skipped");
            return null;
        }
        int stream = (int)Num(ev, "stream", 0);
        double length = Num(ev, "length", 0) * Num(ev, "rate", 1);
        Media media;
        if (Bool(ev, "reverse", false)) {
            // Milan's offset of a reversed event is the end of the span it plays; VEGAS reverses through a subclip.
            double start = Math.Max(0, offset - length);
            media = new Subclip(proj, path, Tc(start), Tc(length), true, Str(ev, "name"));
            offset = 0;
        } else {
            media = proj.MediaPool.Find(path);
            if (media == null) {
                media = proj.MediaPool.AddMedia(path);
            }
        }
        if (media == null) {
            notes.Add(at + ": " + path + " could not be opened; the event was skipped");
            return null;
        }
        MediaStream ms = video ? (MediaStream)media.GetVideoStreamByIndex(stream) : (MediaStream)media.GetAudioStreamByIndex(stream);
        if (ms == null && stream > 0) {
            notes.Add(at + ": " + path + " has no stream " + (stream + 1) + "; the first is used");
            ms = video ? (MediaStream)media.GetVideoStreamByIndex(0) : (MediaStream)media.GetAudioStreamByIndex(0);
        }
        if (ms == null) {
            notes.Add(at + ": " + path + " has no " + (video ? "video" : "audio") + "; the event was skipped");
        }
        return ms;
    }

    void CommonEvent(TrackEvent ev, Dictionary<string, object> e, MediaStream ms, double offset, string at)
    {
        Take take = ev.AddTake(ms, true, Str(e, "name"));
        take.Offset = Tc(offset);
        double rate = Num(e, "rate", 1);
        if (rate > 0 && Math.Abs(rate - 1) > 1e-9) {
            ev.PlaybackRate = rate;
        }
        ev.FadeIn.Length = Tc(Num(e, "fade_in", 0));
        ev.FadeOut.Length = Tc(Num(e, "fade_out", 0));
        ev.FadeIn.Curve = (CurveType)Enum(typeof(CurveType), Str(e, "fade_in_curve"), CurveType.Smooth);
        ev.FadeOut.Curve = (CurveType)Enum(typeof(CurveType), Str(e, "fade_out_curve"), CurveType.Smooth);
        ev.FadeIn.Gain = (float)Num(e, "gain", 1);
        ev.Mute = Bool(e, "mute", false);
        ev.Locked = Bool(e, "locked", false);
        ev.Loop = Bool(e, "loop", true);
        foreach (object t in List(e, "takes")) {
            Dictionary<string, object> tk = t as Dictionary<string, object>;
            if (tk == null || Str(tk, "path").Length == 0) {
                continue;
            }
            try {
                Media m = proj.MediaPool.Find(Str(tk, "path"));
                if (m == null) {
                    m = proj.MediaPool.AddMedia(Str(tk, "path"));
                }
                int stream = (int)Num(tk, "stream", 0);
                MediaStream tms = ev.IsVideo() ? (MediaStream)m.GetVideoStreamByIndex(stream) : (MediaStream)m.GetAudioStreamByIndex(stream);
                if (tms != null) {
                    Take other = ev.AddTake(tms, false, Str(tk, "name"));
                    other.Offset = Tc(Num(tk, "offset", 0));
                }
            } catch (Exception) {
                notes.Add(at + ": take " + Str(tk, "path") + " could not be opened");
            }
        }
        object g = e.ContainsKey("group") ? e["group"] : null;
        if (g is double) {
            long key = (long)(double)g;
            TrackEventGroup grp;
            if (!groups.TryGetValue(key, out grp)) {
                grp = new TrackEventGroup(proj);
                proj.Groups.Add(grp);
                groups[key] = grp;
            }
            grp.Add(ev);
        }
        SetTransition(ev.FadeIn, Obj(e, "transition_in"), at);
        SetTransition(ev.FadeOut, Obj(e, "transition_out"), at);
    }

    void AddVideoEvent(VideoTrack vt, Dictionary<string, object> e, string track)
    {
        string at = track + ": the event at " + Num(e, "start", 0).ToString("0.###", CultureInfo.InvariantCulture) + " s";
        try {
            double offset;
            MediaStream ms = StreamFor(e, true, at, out offset);
            if (ms == null) {
                return;
            }
            VideoEvent ve = vt.AddVideoEvent(Tc(Num(e, "start", 0)), Tc(Num(e, "length", 1)));
            CommonEvent(ve, e, ms, offset, at);
            // Pan/Crop: the first key exists; the rest are added.
            List<object> keys = List(e, "pan_crop");
            for (int i = 0; i < keys.Count; i++) {
                Dictionary<string, object> k = keys[i] as Dictionary<string, object>;
                if (k == null) {
                    continue;
                }
                VideoMotionKeyframe vk;
                if (i == 0) {
                    vk = ve.VideoMotion.Keyframes[0];
                } else {
                    vk = new VideoMotionKeyframe(proj, Tc(Num(k, "t", 0)));
                    ve.VideoMotion.Keyframes.Add(vk);
                }
                double[] tl = Pair(k, "tl"), tr = Pair(k, "tr"), bl = Pair(k, "bl");
                if (tl != null && tr != null && bl != null) {
                    // The fourth corner closes the parallelogram.
                    double brx = tr[0] + bl[0] - tl[0], bry = tr[1] + bl[1] - tl[1];
                    vk.Bounds = new VideoMotionBounds((float)tl[0], (float)tl[1], (float)tr[0], (float)tr[1], (float)brx, (float)bry, (float)bl[0], (float)bl[1]);
                }
                vk.Type = KeyType(Str(k, "curve"));
            }
            AddVideoFx(ve.Effects, List(e, "fx"), at + ": event FX");
            foreach (object o in List(e, "envelopes")) {
                Dictionary<string, object> env = o as Dictionary<string, object>;
                if (env == null) {
                    continue;
                }
                if (Str(env, "kind") != "Velocity") {
                    notes.Add(at + ": the " + Str(env, "kind") + " envelope was left out");
                    continue;
                }
                Envelope velocity = new Envelope(EnvelopeType.Velocity);
                ve.Envelopes.Add(velocity);
                FillPoints(velocity, List(env, "points"));
            }
        } catch (Exception ex) {
            notes.Add(at + ": " + ex.Message);
        }
    }

    void AddAudioEvent(AudioTrack at_, Dictionary<string, object> e, string track)
    {
        string at = track + ": the event at " + Num(e, "start", 0).ToString("0.###", CultureInfo.InvariantCulture) + " s";
        try {
            double offset;
            MediaStream ms = StreamFor(e, false, at, out offset);
            if (ms == null) {
                return;
            }
            AudioEvent ae = at_.AddAudioEvent(Tc(Num(e, "start", 0)), Tc(Num(e, "length", 1)));
            CommonEvent(ae, e, ms, offset, at);
            ae.Normalize = Bool(e, "normalize", false);
            List<object> fx = List(e, "fx");
            if (fx.Count > 0) {
                AddAudioFx(ae.Effects, fx, at + ": event FX");
            }
        } catch (Exception ex) {
            notes.Add(at + ": " + ex.Message);
        }
    }

    // ---- Envelopes, Track Motion ----

    void AddEnvelopes(Envelopes envelopes, List<object> list, Track track, string at)
    {
        foreach (object o in list) {
            Dictionary<string, object> env = o as Dictionary<string, object>;
            if (env == null) {
                continue;
            }
            string kind = Str(env, "kind");
            EnvelopeType type = (EnvelopeType)Enum(typeof(EnvelopeType), kind, EnvelopeType.Unknown);
            if (type == EnvelopeType.Unknown || !track.CanAddEnvelope(type)) {
                notes.Add(at + ": the " + kind + " envelope was left out");
                continue;
            }
            Envelope e = envelopes.FindByType(type);
            if (e == null) {
                e = new Envelope(type);
                envelopes.Add(e);
            }
            FillPoints(e, List(env, "points"));
        }
    }

    // Points into an envelope: the first point exists at 0 and is moved; the rest are added.
    static void FillPoints(Envelope e, List<object> points)
    {
        bool first = true;
        foreach (object o in points) {
            Dictionary<string, object> p = o as Dictionary<string, object>;
            if (p == null) {
                continue;
            }
            Timecode x = Tc(Num(p, "t", 0));
            double y = Num(p, "v", e.Neutral);
            CurveType curve = (CurveType)Enum(typeof(CurveType), Str(p, "curve"), CurveType.Linear);
            EnvelopePoint pt = first && e.Points.Count > 0 ? e.Points[0] : null;
            first = false;
            if (pt != null) {
                pt.X = x;
                pt.Y = y;
                pt.Curve = curve;
            } else {
                e.Points.Add(new EnvelopePoint(x, y, curve));
            }
        }
    }

    // Track Motion keys in VEGAS's units (pixels from the frame centre, y up, degrees), as the export writes them.
    void SetMotion(TrackMotion tm, List<object> keys)
    {
        for (int i = 0; i < keys.Count; i++) {
            Dictionary<string, object> k = keys[i] as Dictionary<string, object>;
            if (k == null) {
                continue;
            }
            TrackMotionKeyframe mk = i == 0 && tm.MotionKeyframes.Count > 0 ? tm.MotionKeyframes[0] : tm.InsertMotionKeyframe(Tc(Num(k, "t", 0)));
            mk.PositionX = Num(k, "x", 0);
            mk.PositionY = Num(k, "y", 0);
            mk.PositionZ = Num(k, "z", 0);
            double w = Num(k, "w", 0), h = Num(k, "h", 0);
            if (w > 0 && h > 0) {
                mk.Width = w;
                mk.Height = h;
            }
            mk.RotationX = Num(k, "rx", 0);
            mk.RotationY = Num(k, "ry", 0);
            mk.RotationZ = Num(k, "rz", 0);
            mk.Type = KeyType(Str(k, "curve"));
        }
    }

    // ---- FX ----

    static string FxLabel(Dictionary<string, object> fx)
    {
        string name = Str(fx, "name"), id = Str(fx, "id");
        return name.Length > 0 ? name : id;
    }

    // The plug-in an exported FX names: by UniqueID ("{Svfx:com.vendor:name}"), then by name.
    static PlugInNode FindPlugIn(PlugInNode root, Dictionary<string, object> fx)
    {
        string id = Str(fx, "id");
        PlugInNode plug = null;
        if (id.Length > 0) {
            plug = root.GetChildByUniqueID(id);
            if (plug == null && !id.StartsWith("{")) {
                plug = root.GetChildByUniqueID("{Svfx:" + id + "}");
            }
        }
        string name = Str(fx, "name");
        if (plug == null && name.Length > 0) {
            plug = root.GetChildByName(name);
            if (plug == null && !name.StartsWith("VEGAS ")) {
                plug = root.GetChildByName("VEGAS " + name);
            }
        }
        return plug;
    }

    void AddVideoFx(Effects effects, List<object> list, string at)
    {
        foreach (object o in list) {
            Dictionary<string, object> fx = o as Dictionary<string, object>;
            if (fx == null) {
                continue;
            }
            PlugInNode plug = FindPlugIn(vegas.VideoFX, fx);
            if (plug == null) {
                notes.Add(at + ": " + FxLabel(fx) + " is not installed");
                continue;
            }
            try {
                Effect e = effects.AddEffect(plug);
                e.Bypass = Bool(fx, "bypass", false);
                SetParams(e.OFXEffect, List(fx, "params"));
            } catch (Exception ex) {
                notes.Add(at + ": " + FxLabel(fx) + ": " + ex.Message);
            }
        }
    }

    // Audio FX by name (VEGAS's audio plug-ins have no scriptable parameters; a preset is applied by name).
    void AddAudioFx(Effects effects, List<object> list, string at)
    {
        foreach (object o in list) {
            Dictionary<string, object> fx = o as Dictionary<string, object>;
            if (fx == null) {
                continue;
            }
            PlugInNode plug = FindPlugIn(vegas.AudioFX, fx);
            if (plug == null) {
                notes.Add(at + ": audio FX " + FxLabel(fx) + " is not installed");
                continue;
            }
            try {
                Effect e = effects.AddEffect(plug);
                e.Bypass = Bool(fx, "bypass", false);
                string preset = Str(fx, "preset");
                if (preset.Length > 0) {
                    try {
                        e.Preset = preset;
                    } catch (Exception) {
                        notes.Add(at + ": preset " + preset + " of " + FxLabel(fx) + " was not found");
                    }
                }
            } catch (Exception ex) {
                notes.Add(at + ": " + FxLabel(fx) + ": " + ex.Message);
            }
        }
    }

    void SetTransition(Fade fade, Dictionary<string, object> fx, string at)
    {
        if (fx == null) {
            return;
        }
        PlugInNode plug = FindPlugIn(vegas.Transitions, fx);
        if (plug == null) {
            notes.Add(at + ": transition " + FxLabel(fx) + " is not installed; a crossfade is used");
            return;
        }
        try {
            Effect e = new Effect(plug);
            fade.Transition = e;
            Effect set = fade.Transition as Effect;
            if (set != null) {
                SetParams(set.OFXEffect, List(fx, "params"));
            }
        } catch (Exception ex) {
            notes.Add(at + ": transition " + FxLabel(fx) + ": " + ex.Message);
        }
    }

    // Every exported parameter the plug-in has, by name, with its keyframes.
    static void SetParams(OFXEffect ofx, List<object> list)
    {
        if (ofx == null) {
            return;
        }
        foreach (object o in list) {
            Dictionary<string, object> p = o as Dictionary<string, object>;
            if (p == null) {
                continue;
            }
            OFXParameter prm = ofx.FindParameterByName(Str(p, "name"));
            if (prm == null) {
                continue;
            }
            try {
                SetParam(prm, p.ContainsKey("v") ? p["v"] : null, List(p, "keys"));
            } catch (Exception) {
                // A value the parameter refuses (out of range, wrong shape) keeps the plug-in's default.
            }
        }
    }

    static void SetParam(OFXParameter prm, object v, List<object> keys)
    {
        switch (prm.ParameterType) {
            case OFXParameterType.Double: {
                OFXDoubleParameter d = (OFXDoubleParameter)prm;
                d.Value = ToNum(v, d.Value);
                if (keys.Count > 1) {
                    d.IsAnimated = true;
                    foreach (object o in keys) {
                        Dictionary<string, object> k = o as Dictionary<string, object>;
                        if (k != null) {
                            d.SetValueAtTime(Tc(Num(k, "t", 0)), ToNum(k.ContainsKey("v") ? k["v"] : null, d.Value));
                        }
                    }
                    foreach (OFXDoubleKeyframe kf in d.Keyframes) {
                        kf.Interpolation = Interp(CurveAt(keys, kf.Time));
                    }
                }
                break;
            }
            case OFXParameterType.Double2D: {
                OFXDouble2DParameter d = (OFXDouble2DParameter)prm;
                d.Value = To2D(v, d.Value);
                if (keys.Count > 1) {
                    d.IsAnimated = true;
                    foreach (object o in keys) {
                        Dictionary<string, object> k = o as Dictionary<string, object>;
                        if (k != null) {
                            d.SetValueAtTime(Tc(Num(k, "t", 0)), To2D(k.ContainsKey("v") ? k["v"] : null, d.Value));
                        }
                    }
                    foreach (OFXDouble2DKeyframe kf in d.Keyframes) {
                        kf.Interpolation = Interp(CurveAt(keys, kf.Time));
                    }
                }
                break;
            }
            case OFXParameterType.Double3D: {
                OFXDouble3DParameter d = (OFXDouble3DParameter)prm;
                d.Value = To3D(v, d.Value);
                break;
            }
            case OFXParameterType.Integer: {
                OFXIntegerParameter d = (OFXIntegerParameter)prm;
                d.Value = (int)Math.Round(ToNum(v, d.Value));
                break;
            }
            case OFXParameterType.Boolean: {
                OFXBooleanParameter d = (OFXBooleanParameter)prm;
                if (v is bool) {
                    d.Value = (bool)v;
                } else if (v is double) {
                    d.Value = (double)v >= 0.5;
                }
                break;
            }
            case OFXParameterType.Choice: {
                OFXChoiceParameter d = (OFXChoiceParameter)prm;
                int index = (int)Math.Round(ToNum(v, -1));
                OFXChoice[] choices = d.Choices;
                if (choices != null && index >= 0 && index < choices.Length) {
                    d.Value = choices[index];
                }
                break;
            }
            case OFXParameterType.RGB: {
                OFXRGBParameter d = (OFXRGBParameter)prm;
                d.Value = ToColor(v, d.Value, false);
                break;
            }
            case OFXParameterType.RGBA: {
                OFXRGBAParameter d = (OFXRGBAParameter)prm;
                d.Value = ToColor(v, d.Value, true);
                break;
            }
            case OFXParameterType.String: {
                if (v is string) {
                    ((OFXStringParameter)prm).Value = (string)v;
                }
                break;
            }
            case OFXParameterType.Custom: {
                if (v is string) {
                    ((OFXCustomParameter)prm).Value = (string)v;
                }
                break;
            }
            default:
                break;
        }
    }

    static string CurveAt(List<object> keys, Timecode t)
    {
        double at = t.ToMilliseconds() / 1000.0;
        foreach (object o in keys) {
            Dictionary<string, object> k = o as Dictionary<string, object>;
            if (k != null && Math.Abs(Num(k, "t", 0) - at) < 0.0005) {
                return Str(k, "curve");
            }
        }
        return "Linear";
    }

    static OFXInterpolationType Interp(string name)
    {
        if (name == "None") {
            return OFXInterpolationType.Hold;
        }
        return (OFXInterpolationType)Enum(typeof(OFXInterpolationType), name, OFXInterpolationType.Linear);
    }

    static VideoKeyframeType KeyType(string name)
    {
        if (name == "None") {
            return VideoKeyframeType.Hold;
        }
        return (VideoKeyframeType)Enum(typeof(VideoKeyframeType), name, VideoKeyframeType.Smooth);
    }

    static double ToNum(object v, double fallback)
    {
        if (v is double) {
            return (double)v;
        }
        if (v is bool) {
            return (bool)v ? 1 : 0;
        }
        List<object> a = v as List<object>;
        if (a != null && a.Count > 0 && a[0] is double) {
            return (double)a[0];
        }
        return fallback;
    }

    static double[] Nums(object v)
    {
        List<object> a = v as List<object>;
        if (a == null) {
            return null;
        }
        double[] r = new double[a.Count];
        for (int i = 0; i < a.Count; i++) {
            r[i] = a[i] is double ? (double)a[i] : 0;
        }
        return r;
    }

    static OFXDouble2D To2D(object v, OFXDouble2D fallback)
    {
        double[] a = Nums(v);
        if (a == null || a.Length < 2) {
            return fallback;
        }
        OFXDouble2D r = new OFXDouble2D();
        r.X = a[0];
        r.Y = a[1];
        return r;
    }

    static OFXDouble3D To3D(object v, OFXDouble3D fallback)
    {
        double[] a = Nums(v);
        if (a == null || a.Length < 3) {
            return fallback;
        }
        OFXDouble3D r = new OFXDouble3D();
        r.X = a[0];
        r.Y = a[1];
        r.Z = a[2];
        return r;
    }

    static OFXColor ToColor(object v, OFXColor fallback, bool alpha)
    {
        double[] a = Nums(v);
        if (a == null || a.Length < 3) {
            return fallback;
        }
        return alpha ? new OFXColor(a[0], a[1], a[2], a.Length > 3 ? a[3] : 1.0) : new OFXColor(a[0], a[1], a[2]);
    }

    // ---- JSON access ----

    static Timecode Tc(double seconds)
    {
        return Timecode.FromSeconds(seconds);
    }

    static object Enum(Type type, string name, object fallback)
    {
        if (String.IsNullOrEmpty(name)) {
            return fallback;
        }
        try {
            return System.Enum.Parse(type, name, true);
        } catch (Exception) {
            return fallback;
        }
    }

    static string Str(Dictionary<string, object> o, string key)
    {
        object v;
        return o != null && o.TryGetValue(key, out v) && v is string ? (string)v : "";
    }

    static double Num(Dictionary<string, object> o, string key, double fallback)
    {
        object v;
        return o != null && o.TryGetValue(key, out v) && v is double ? (double)v : fallback;
    }

    static bool Bool(Dictionary<string, object> o, string key, bool fallback)
    {
        object v;
        return o != null && o.TryGetValue(key, out v) && v is bool ? (bool)v : fallback;
    }

    static List<object> List(Dictionary<string, object> o, string key)
    {
        object v;
        List<object> l = o != null && o.TryGetValue(key, out v) ? v as List<object> : null;
        return l ?? new List<object>();
    }

    static Dictionary<string, object> Obj(Dictionary<string, object> o, string key)
    {
        object v;
        return o != null && o.TryGetValue(key, out v) ? v as Dictionary<string, object> : null;
    }

    static double[] Pair(Dictionary<string, object> o, string key)
    {
        double[] a = o != null && o.ContainsKey(key) ? Nums(o[key]) : null;
        return a != null && a.Length >= 2 ? a : null;
    }
}

// A small JSON reader (the script host has no JSON library): objects become Dictionary<string, object>, arrays
// List<object>, numbers double, plus string, bool and null.
public static class Json
{
    public static object Parse(string s)
    {
        int i = 0;
        object v = Value(s, ref i);
        Skip(s, ref i);
        return v;
    }

    static void Skip(string s, ref int i)
    {
        while (i < s.Length && (s[i] == ' ' || s[i] == '\t' || s[i] == '\r' || s[i] == '\n' || s[i] == '﻿')) {
            i++;
        }
    }

    static object Value(string s, ref int i)
    {
        Skip(s, ref i);
        if (i >= s.Length) {
            throw new Exception("unexpected end of JSON");
        }
        char c = s[i];
        if (c == '{') {
            Dictionary<string, object> o = new Dictionary<string, object>();
            i++;
            Skip(s, ref i);
            if (i < s.Length && s[i] == '}') {
                i++;
                return o;
            }
            while (true) {
                Skip(s, ref i);
                string key = Str(s, ref i);
                Skip(s, ref i);
                Expect(s, ref i, ':');
                o[key] = Value(s, ref i);
                Skip(s, ref i);
                if (i < s.Length && s[i] == ',') {
                    i++;
                    continue;
                }
                Expect(s, ref i, '}');
                return o;
            }
        }
        if (c == '[') {
            List<object> a = new List<object>();
            i++;
            Skip(s, ref i);
            if (i < s.Length && s[i] == ']') {
                i++;
                return a;
            }
            while (true) {
                a.Add(Value(s, ref i));
                Skip(s, ref i);
                if (i < s.Length && s[i] == ',') {
                    i++;
                    continue;
                }
                Expect(s, ref i, ']');
                return a;
            }
        }
        if (c == '"') {
            return Str(s, ref i);
        }
        if (s.Length - i >= 4 && s.Substring(i, 4) == "true") {
            i += 4;
            return true;
        }
        if (s.Length - i >= 5 && s.Substring(i, 5) == "false") {
            i += 5;
            return false;
        }
        if (s.Length - i >= 4 && s.Substring(i, 4) == "null") {
            i += 4;
            return null;
        }
        int start = i;
        while (i < s.Length && "+-0123456789.eE".IndexOf(s[i]) >= 0) {
            i++;
        }
        if (start == i) {
            throw new Exception("unexpected character '" + c + "' in JSON at " + i);
        }
        return Double.Parse(s.Substring(start, i - start), NumberStyles.Float, CultureInfo.InvariantCulture);
    }

    static void Expect(string s, ref int i, char c)
    {
        if (i >= s.Length || s[i] != c) {
            throw new Exception("expected '" + c + "' in JSON at " + i);
        }
        i++;
    }

    static string Str(string s, ref int i)
    {
        Expect(s, ref i, '"');
        StringBuilder b = new StringBuilder();
        while (i < s.Length) {
            char c = s[i++];
            if (c == '"') {
                return b.ToString();
            }
            if (c != '\\') {
                b.Append(c);
                continue;
            }
            if (i >= s.Length) {
                break;
            }
            char e = s[i++];
            switch (e) {
                case 'n': b.Append('\n'); break;
                case 't': b.Append('\t'); break;
                case 'r': b.Append('\r'); break;
                case 'b': b.Append('\b'); break;
                case 'f': b.Append('\f'); break;
                case 'u':
                    b.Append((char)Int32.Parse(s.Substring(i, 4), NumberStyles.HexNumber, CultureInfo.InvariantCulture));
                    i += 4;
                    break;
                default: b.Append(e); break;
            }
        }
        throw new Exception("unterminated string in JSON");
    }
}
