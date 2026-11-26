// Export to Milan Pro (VEGAS Pro 13 and earlier; use "Export to Milan Pro.cs" in VEGAS Pro 14 and later).
//
// Writes the open project as <name>.milan.json (format "milan-vegas", version 2), which Milan Pro opens with
// File > Import > VEGAS Project... Run it with Tools > Scripting > Run Script..., or copy it into VEGAS's "Script Menu"
// folder to get a menu item.
// Exported: tracks, events (in points, rate, fades and curves, gain/opacity, mute, lock, loop, normalize, groups,
// takes, stream), track volume, pan, Pan/Crop, subclips and reversed events, composite level and mode, parent/child
// compositing, Track Motion keyframes (and whether 2D Shadow / Glow are on), track and event envelopes (velocity
// included), event / track / media / video output FX with every OFX parameter and its keyframes, transitions,
// generated media, audio FX by name and preset, markers, regions, the time selection, and the project's video and
// audio settings. Milan Pro maps the VEGAS plug-ins onto its own and lists what it cannot use when it imports the
// file (docs/INTERCHANGE.md).
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
    List<string> unsupported = new List<string>();
    Dictionary<TrackEvent, int> groups = new Dictionary<TrackEvent, int>();
    // Media FX are exported once per file.
    Dictionary<string, string> mediaFx = new Dictionary<string, string>();
    List<string> mediaFxOrder = new List<string>();

    public void FromVegas(Vegas v)
    {
        vegas = v;
        try {
            Project proj = vegas.Project;
            string name = String.IsNullOrEmpty(proj.FilePath) ? "Untitled" : Path.GetFileNameWithoutExtension(proj.FilePath);
            SaveFileDialog dlg = new SaveFileDialog();
            dlg.Title = "Export to Milan Pro";
            dlg.Filter = "Milan Pro import (*.milan.json)|*.milan.json";
            dlg.FileName = name + ".milan.json";
            if (!String.IsNullOrEmpty(proj.FilePath)) {
                dlg.InitialDirectory = Path.GetDirectoryName(proj.FilePath);
            }
            if (dlg.ShowDialog() != DialogResult.OK) {
                return;
            }

            int g = 0;
            foreach (TrackEventGroup grp in proj.Groups) {
                foreach (TrackEvent ev in grp) {
                    groups[ev] = g;
                }
                g++;
            }
            List<string> tracks = new List<string>();
            foreach (Track track in proj.Tracks) {
                tracks.Add(TrackJson(track, proj));
            }
            List<string> markers = new List<string>();
            foreach (Marker m in proj.Markers) {
                markers.Add(Obj("t", N(S(m.Position)), "name", Q(m.Label)));
            }
            List<string> regions = new List<string>();
            foreach (Region r in proj.Regions) {
                regions.Add(Obj("t", N(S(r.Position)), "length", N(S(r.Length)), "name", Q(r.Label)));
            }
            List<string> media = new List<string>();
            foreach (string path in mediaFxOrder) {
                media.Add(Obj("path", Q(path), "fx", mediaFx[path]));
            }
            List<string> notes = new List<string>();
            foreach (string s in unsupported) {
                notes.Add(Q(s));
            }
            string json = Obj(
                "format", Q("milan-vegas"),
                "version", "2",
                "name", Q(name),
                "project", Obj("w", proj.Video.Width.ToString(CultureInfo.InvariantCulture),
                               "h", proj.Video.Height.ToString(CultureInfo.InvariantCulture),
                               "fps", N(proj.Video.FrameRate),
                               "sample_rate", proj.Audio.SampleRate.ToString(CultureInfo.InvariantCulture),
                               "field_order", Q(proj.Video.FieldOrder.ToString()),
                               "pixel_aspect", N(proj.Video.PixelAspectRatio),
                               "quality", Q(proj.Video.RenderQuality.ToString()),
                               "motion_blur", Q(proj.Video.MotionBlurType.ToString()),
                               "deinterlace", Q(proj.Video.DeinterlaceMethod.ToString()),
                               "pixel_format", Q(proj.Video.PixelFormat.ToString()),
                               "bit_depth", proj.Audio.BitDepth.ToString(CultureInfo.InvariantCulture),
                               "bus_mode", Q(proj.Audio.MasterBusMode.ToString())),
                "tracks", Arr(tracks),
                "markers", Arr(markers),
                "regions", Arr(regions),
                "loop", Obj("t", N(S(vegas.SelectionStart)), "length", N(S(vegas.SelectionLength))),
                "output_fx", FxList(proj.VideoBus.Effects),
                "master_fx", FxList(proj.MasterBus.Effects),
                "media_fx", Arr(media),
                "unsupported", Arr(notes));
            File.WriteAllText(dlg.FileName, json, new UTF8Encoding(false));
            string done = "Exported to " + dlg.FileName + "\n\nIn Milan Pro, choose File > Import > VEGAS Project... and pick this file.";
            if (unsupported.Count > 0) {
                done += "\n\nMilan Pro will list " + unsupported.Count + " item(s) it cannot import.";
            }
            MessageBox.Show(done, "Export to Milan Pro");
        } catch (Exception e) {
            MessageBox.Show("Export failed: " + e.Message, "Export to Milan Pro");
        }
    }

    string TrackJson(Track track, Project proj)
    {
        string where = "Track " + (track.Index + 1);
        List<string> events = new List<string>();
        foreach (TrackEvent ev in track.Events) {
            string e = EventJson(ev, where);
            if (e != null) {
                events.Add(e);
            }
        }
        string common = Obj("name", Q(track.Name), "mute", B(track.Mute), "solo", B(track.Solo), "envelopes", EnvJson(track.Envelopes),
                            "fx", FxList(track.Effects), "events", Arr(events));
        if (track.IsVideo()) {
            VideoTrack vt = (VideoTrack)track;
            TrackMotion tm = vt.TrackMotion;
            if (vt.CompositeMode == CompositeMode.Custom) {
                unsupported.Add(where + ": Custom compositing mode (its compositor is not exported)");
            }
            string parentMotion = vt.IsCompositingParent ? MotionJson(vt.ParentTrackMotion) : "[]";
            string video = Obj("kind", Q("video"), "level", N(vt.CompositeLevel), "mode", Q(vt.CompositeMode.ToString()),
                               "child", B(vt.IsCompositingChild), "parent", B(vt.IsCompositingParent),
                               "parent_mode", Q(vt.ParentCompositeMode.ToString()), "parent_motion", parentMotion,
                               "motion", MotionJson(tm), "shadow", B(tm.ShadowEnabled), "glow", B(tm.GlowEnabled));
            return Join(video, common);
        }
        AudioTrack at = (AudioTrack)track;
        return Join(Obj("kind", Q("audio"), "volume", N(at.Volume), "pan", N(at.PanX)), common);
    }

    string EventJson(TrackEvent ev, string where)
    {
        string at = where + ", event at " + ev.Start.ToString();
        Take take = ev.ActiveTake;
        if (take == null || take.Media == null) {
            unsupported.Add(at + ": an event without media was skipped");
            return null;
        }
        string generator = "null";
        string path = "";
        bool reverse = false;
        double offset = 0;
        if (take.Media.IsGenerated()) {
            generator = take.Media.Generator == null ? "null" : FxJson(take.Media.Generator);
        } else {
            path = SourcePath(take);
            offset = SourceOffset(take, out reverse);
            if (take.Media.Effects.Count > 0 && !mediaFx.ContainsKey(path)) {
                mediaFx[path] = FxList(take.Media.Effects);
                mediaFxOrder.Add(path);
            }
        }
        List<string> panCrop = new List<string>();
        string srcSize = "null";
        string fx = "[]";
        string envelopes = "[]";
        bool normalize = false;
        if (ev.IsVideo()) {
            VideoEvent ve = (VideoEvent)ev;
            fx = FxList(ve.Effects);
            envelopes = EnvJson(ve.Envelopes);
            VideoStream vs = take.MediaStream as VideoStream;
            if (vs != null) {
                foreach (VideoMotionKeyframe k in ve.VideoMotion.Keyframes) {
                    VideoMotionBounds b = k.Bounds;
                    panCrop.Add(Obj("t", N(S(k.Position)), "tl", V(b.TopLeft), "tr", V(b.TopRight), "bl", V(b.BottomLeft),
                                    "curve", Q(k.Type.ToString())));
                }
                srcSize = Arr(new List<string> { vs.Width.ToString(CultureInfo.InvariantCulture), vs.Height.ToString(CultureInfo.InvariantCulture) });
            } else if (ve.VideoMotion.Keyframes.Count > 1) {
                unsupported.Add(at + ": Pan/Crop keyframes");
            }
            if (ve.VideoMotion.ScaleToFill) {
                unsupported.Add(at + ": Pan/Crop Stretch to fill frame (imported letterboxed)");
            }
        } else {
            AudioEvent ae = (AudioEvent)ev;
            fx = FxList(ae.Effects);
            normalize = ae.Normalize;
        }
        List<string> takes = new List<string>();
        foreach (Take t in ev.Takes) {
            if (t.IsActive || t.Media == null) {
                continue;
            }
            if (t.Media.IsGenerated()) {
                unsupported.Add(at + ": a generated-media take was skipped");
                continue;
            }
            bool rev;
            double off = SourceOffset(t, out rev);
            takes.Add(Obj("path", Q(SourcePath(t)), "offset", N(off), "reverse", B(rev), "name", Q(t.Name),
                          "stream", t.StreamIndex.ToString(CultureInfo.InvariantCulture)));
        }
        int g;
        string group = groups.TryGetValue(ev, out g) ? g.ToString(CultureInfo.InvariantCulture) : "null";
        return Obj(
            "start", N(S(ev.Start)),
            "length", N(S(ev.Length)),
            "offset", N(offset),
            "reverse", B(reverse),
            "rate", N(ev.PlaybackRate),
            "path", Q(path),
            "stream", take.StreamIndex.ToString(CultureInfo.InvariantCulture),
            "name", Q(take.Name),
            "fade_in", N(S(ev.FadeIn.Length)),
            "fade_out", N(S(ev.FadeOut.Length)),
            "fade_in_curve", Q(ev.FadeIn.Curve.ToString()),
            "fade_out_curve", Q(ev.FadeOut.Curve.ToString()),
            "gain", N(ev.FadeIn.Gain),
            "mute", B(ev.Mute),
            "locked", B(ev.Locked),
            "loop", B(ev.Loop),
            "normalize", B(normalize),
            "group", group,
            "takes", Arr(takes),
            "pan_crop", Arr(panCrop),
            "src_size", srcSize,
            "fx", fx,
            "envelopes", envelopes,
            "transition_in", TransitionJson(ev.FadeIn),
            "transition_out", TransitionJson(ev.FadeOut),
            "generator", generator);
    }

    // A subclip's take offset counts from the subclip's start, or from its end when it is reversed; this is the
    // source file's time at the event start.
    static double SourceOffset(Take t, out bool reversed)
    {
        Subclip sub = t.Media as Subclip;
        reversed = sub != null && sub.IsReversed;
        if (sub == null) {
            return S(t.Offset);
        }
        return reversed ? S(sub.Start) + S(sub.Length) - S(t.Offset) : S(sub.Start) + S(t.Offset);
    }

    static string SourcePath(Take t)
    {
        Subclip sub = t.Media as Subclip;
        return sub != null && sub.ParentMedia != null ? sub.ParentMedia.FilePath : t.MediaPath;
    }

    // ---- Envelopes, Track Motion ----

    static string EnvJson(Envelopes envelopes)
    {
        List<string> envs = new List<string>();
        foreach (Envelope env in envelopes) {
            List<string> pts = new List<string>();
            foreach (EnvelopePoint p in env.Points) {
                pts.Add(Obj("t", N(S(p.X)), "v", N(p.Y), "curve", Q(p.Curve.ToString())));
            }
            envs.Add(Obj("kind", Q(env.Type.ToString()), "neutral", N(env.Neutral), "points", Arr(pts)));
        }
        return Arr(envs);
    }

    // Track Motion keyframes in VEGAS's units: pixels from the frame centre (y up), size in pixels, degrees.
    static string MotionJson(TrackMotion tm)
    {
        List<string> keys = new List<string>();
        foreach (TrackMotionKeyframe k in tm.MotionKeyframes) {
            keys.Add(Obj("t", N(S(k.Position)), "x", N(k.PositionX), "y", N(k.PositionY), "z", N(k.PositionZ),
                         "w", N(k.Width), "h", N(k.Height), "rx", N(k.RotationX), "ry", N(k.RotationY), "rz", N(k.RotationZ),
                         "ox", N(k.OrientationX), "oy", N(k.OrientationY), "oz", N(k.OrientationZ), "curve", Q(k.Type.ToString())));
        }
        return Arr(keys);
    }

    // ---- FX ----

    static string FxList(Effects list)
    {
        List<string> items = new List<string>();
        foreach (Effect fx in list) {
            items.Add(FxJson(fx));
        }
        return Arr(items);
    }

    static string TransitionJson(Fade f)
    {
        Effect tr = f.Transition as Effect;
        return tr == null ? "null" : FxJson(tr);
    }

    // A plug-in instance: id (PlugIn.UniqueID, "{Svfx:com.vendor:name}" for OFX), name, bypass, preset and, for OFX,
    // every parameter with its keyframes. Audio (DirectX / VST) FX have no readable parameters: name and preset only.
    static string FxJson(Effect fx)
    {
        List<string> ps = new List<string>();
        OFXEffect ofx = null;
        try {
            ofx = fx.OFXEffect;
        } catch (Exception) {
            ofx = null;
        }
        if (ofx != null) {
            foreach (OFXParameter prm in ofx.Parameters) {
                string p = null;
                try {
                    p = ParamJson(prm);
                } catch (Exception) {
                    p = null; // a parameter the API cannot read is left out; the rest still export
                }
                if (p != null) {
                    ps.Add(p);
                }
            }
        }
        string preset = "";
        try {
            preset = fx.CurrentPreset != null ? fx.CurrentPreset.Name : "";
        } catch (Exception) {
            preset = ""; // an audio FX without a preset list
        }
        string id = fx.PlugIn != null ? fx.PlugIn.UniqueID : "";
        return Obj("id", Q(id), "name", Q(FxName(fx)), "bypass", B(fx.Bypass), "preset", Q(preset), "params", Arr(ps));
    }

    // One OFX parameter: name, type, value (a number, bool, string or array) and keyframes (event-relative seconds).
    static string ParamJson(OFXParameter prm)
    {
        string type = prm.ParameterType.ToString();
        string value;
        List<string> keys = new List<string>();
        switch (prm.ParameterType) {
            case OFXParameterType.Double: {
                OFXDoubleParameter d = (OFXDoubleParameter)prm;
                value = N(d.Value);
                if (d.IsAnimated) {
                    foreach (OFXDoubleKeyframe k in d.Keyframes) {
                        keys.Add(Key(k.Time, N(k.Value), k.Interpolation));
                    }
                }
                break;
            }
            case OFXParameterType.Double2D: {
                OFXDouble2DParameter d = (OFXDouble2DParameter)prm;
                value = Pair(d.Value.X, d.Value.Y);
                if (d.IsAnimated) {
                    foreach (OFXDouble2DKeyframe k in d.Keyframes) {
                        keys.Add(Key(k.Time, Pair(k.Value.X, k.Value.Y), k.Interpolation));
                    }
                }
                break;
            }
            case OFXParameterType.Double3D: {
                OFXDouble3DParameter d = (OFXDouble3DParameter)prm;
                value = Triple(d.Value.X, d.Value.Y, d.Value.Z);
                if (d.IsAnimated) {
                    foreach (OFXDouble3DKeyframe k in d.Keyframes) {
                        keys.Add(Key(k.Time, Triple(k.Value.X, k.Value.Y, k.Value.Z), k.Interpolation));
                    }
                }
                break;
            }
            case OFXParameterType.Integer: {
                OFXIntegerParameter d = (OFXIntegerParameter)prm;
                value = d.Value.ToString(CultureInfo.InvariantCulture);
                if (d.IsAnimated) {
                    foreach (OFXIntegerKeyframe k in d.Keyframes) {
                        keys.Add(Key(k.Time, k.Value.ToString(CultureInfo.InvariantCulture), k.Interpolation));
                    }
                }
                break;
            }
            case OFXParameterType.RGB: {
                OFXRGBParameter d = (OFXRGBParameter)prm;
                value = Color(d.Value, false);
                if (d.IsAnimated) {
                    foreach (OFXRGBKeyframe k in d.Keyframes) {
                        keys.Add(Key(k.Time, Color(k.Value, false), k.Interpolation));
                    }
                }
                break;
            }
            case OFXParameterType.RGBA: {
                OFXRGBAParameter d = (OFXRGBAParameter)prm;
                value = Color(d.Value, true);
                if (d.IsAnimated) {
                    foreach (OFXRGBAKeyframe k in d.Keyframes) {
                        keys.Add(Key(k.Time, Color(k.Value, true), k.Interpolation));
                    }
                }
                break;
            }
            case OFXParameterType.Boolean:
                value = B(((OFXBooleanParameter)prm).Value);
                break;
            case OFXParameterType.Choice:
                value = ((OFXChoiceParameter)prm).Value.Index.ToString(CultureInfo.InvariantCulture);
                break;
            case OFXParameterType.String:
                value = Q(((OFXStringParameter)prm).Value);
                break;
            case OFXParameterType.Custom:
                value = Q(((OFXCustomParameter)prm).Value);
                break;
            default:
                return null; // groups, pages, push buttons, parametric curves
        }
        return Obj("name", Q(prm.Name), "type", Q(type), "v", value, "keys", Arr(keys));
    }

    static string Key(Timecode t, string value, OFXInterpolationType curve)
    {
        return Obj("t", N(S(t)), "v", value, "curve", Q(curve.ToString()));
    }

    static string Pair(double x, double y)
    {
        return "[" + N(x) + "," + N(y) + "]";
    }

    static string Triple(double x, double y, double z)
    {
        return "[" + N(x) + "," + N(y) + "," + N(z) + "]";
    }

    static string Color(OFXColor c, bool alpha)
    {
        return alpha ? "[" + N(c.R) + "," + N(c.G) + "," + N(c.B) + "," + N(c.A) + "]" : "[" + N(c.R) + "," + N(c.G) + "," + N(c.B) + "]";
    }

    static string V(VideoMotionVertex v)
    {
        return "[" + N(v.X) + "," + N(v.Y) + "]";
    }

    static string FxName(Effect fx)
    {
        return fx == null ? "unknown" : fx.PlugIn != null ? fx.PlugIn.Name : fx.Description;
    }

    // ---- JSON ----

    static double S(Timecode t)
    {
        return t.ToMilliseconds() / 1000.0;
    }

    static string N(double v)
    {
        return Double.IsNaN(v) || Double.IsInfinity(v) ? "0" : v.ToString("R", CultureInfo.InvariantCulture);
    }

    static string B(bool b)
    {
        return b ? "true" : "false";
    }

    static string Q(string s)
    {
        StringBuilder b = new StringBuilder("\"");
        foreach (char c in s ?? "") {
            if (c == '"' || c == '\\') {
                b.Append('\\').Append(c);
            } else if (c < ' ') {
                b.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
            } else {
                b.Append(c);
            }
        }
        return b.Append('"').ToString();
    }

    // {"k1":v1,"k2":v2,...} from alternating keys and JSON values.
    static string Obj(params string[] kv)
    {
        StringBuilder b = new StringBuilder("{");
        for (int i = 0; i + 1 < kv.Length; i += 2) {
            if (i > 0) {
                b.Append(',');
            }
            b.Append(Q(kv[i])).Append(':').Append(kv[i + 1]);
        }
        return b.Append('}').ToString();
    }

    static string Arr(List<string> items)
    {
        return "[" + String.Join(",", items.ToArray()) + "]";
    }

    // Two objects' members in one object.
    static string Join(string a, string b)
    {
        return a.Substring(0, a.Length - 1) + "," + b.Substring(1);
    }
}
