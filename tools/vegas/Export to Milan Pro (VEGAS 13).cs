// Export to Milan Pro (VEGAS Pro 13 and earlier; use "Export to Milan Pro.cs" in VEGAS Pro 14 and later).
//
// Writes the open project as <name>.milan.json, which Milan Pro opens with File > Import > VEGAS Project...
// Run it with Tools > Scripting > Run Script..., or copy it into VEGAS's "Script Menu" folder to get a menu item.
// Tracks, events (in points, rate, fades and curves, gain/opacity, mute, lock, groups, takes), track volume, pan,
// Pan/Crop, subclips and reversed events, composite level and mode, track envelopes, markers, regions and the time
// selection are exported. What Milan Pro cannot import (FX, generated media, transitions, Track Motion, ...) is listed by Milan Pro
// when it imports the file.
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
    // Media FX are reported once per file.
    Dictionary<string, bool> mediaFxSeen = new Dictionary<string, bool>();

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
            if (proj.VideoBus.Effects.Count > 0) {
                unsupported.Add("Video output FX (" + FxNames(proj.VideoBus.Effects) + ")");
            }
            if (proj.MasterBus.Effects.Count > 0) {
                unsupported.Add("Master bus FX (" + FxNames(proj.MasterBus.Effects) + ")");
            }
            List<string> markers = new List<string>();
            foreach (Marker m in proj.Markers) {
                markers.Add(Obj("t", N(S(m.Position)), "name", Q(m.Label)));
            }
            List<string> regions = new List<string>();
            foreach (Region r in proj.Regions) {
                regions.Add(Obj("t", N(S(r.Position)), "length", N(S(r.Length)), "name", Q(r.Label)));
            }
            List<string> notes = new List<string>();
            foreach (string s in unsupported) {
                notes.Add(Q(s));
            }
            string json = Obj(
                "format", Q("milan-vegas"),
                "version", "1",
                "name", Q(name),
                "project", Obj("w", proj.Video.Width.ToString(CultureInfo.InvariantCulture),
                               "h", proj.Video.Height.ToString(CultureInfo.InvariantCulture),
                               "fps", N(proj.Video.FrameRate),
                               "sample_rate", proj.Audio.SampleRate.ToString(CultureInfo.InvariantCulture)),
                "tracks", Arr(tracks),
                "markers", Arr(markers),
                "regions", Arr(regions),
                "loop", Obj("t", N(S(vegas.SelectionStart)), "length", N(S(vegas.SelectionLength))),
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
        if (track.Effects.Count > 0) {
            unsupported.Add(where + ": track FX (" + FxNames(track.Effects) + ")");
        }
        List<string> envs = new List<string>();
        foreach (Envelope env in track.Envelopes) {
            List<string> pts = new List<string>();
            foreach (EnvelopePoint p in env.Points) {
                pts.Add(Obj("t", N(S(p.X)), "v", N(p.Y), "curve", Q(p.Curve.ToString())));
            }
            envs.Add(Obj("kind", Q(env.Type.ToString()), "neutral", N(env.Neutral), "points", Arr(pts)));
        }
        List<string> events = new List<string>();
        foreach (TrackEvent ev in track.Events) {
            string e = EventJson(ev, where);
            if (e != null) {
                events.Add(e);
            }
        }
        string common = Obj("name", Q(track.Name), "mute", B(track.Mute), "solo", B(track.Solo), "envelopes", Arr(envs), "events", Arr(events));
        if (track.IsVideo()) {
            VideoTrack vt = (VideoTrack)track;
            if (vt.IsCompositingChild) {
                unsupported.Add(where + ": parent/child compositing (imported as an ordinary track)");
            }
            if (HasTrackMotion(vt, proj)) {
                unsupported.Add(where + ": Track Motion (including 3D, shadow and glow)");
            }
            return Join(Obj("kind", Q("video"), "level", N(vt.CompositeLevel), "mode", Q(vt.CompositeMode.ToString())), common);
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
        if (take.Media.IsGenerated()) {
            unsupported.Add(at + ": generated media (" + FxName(take.Media.Generator) + ") was skipped");
            return null;
        }
        bool reverse;
        double offset = SourceOffset(take, out reverse);
        List<string> panCrop = new List<string>();
        string srcSize = "null";
        if (take.Media.Effects.Count > 0 && !mediaFxSeen.ContainsKey(take.MediaPath)) {
            mediaFxSeen[take.MediaPath] = true;
            unsupported.Add("Media FX on " + Path.GetFileName(take.MediaPath) + " (" + FxNames(take.Media.Effects) + ")");
        }
        if (ev.IsVideo()) {
            VideoEvent ve = (VideoEvent)ev;
            if (ve.Effects.Count > 0) {
                unsupported.Add(at + ": event FX (" + FxNames(ve.Effects) + ")");
            }
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
            foreach (Envelope env in ve.Envelopes) {
                unsupported.Add(at + ": " + env.Type.ToString() + " envelope");
            }
        } else {
            AudioEvent ae = (AudioEvent)ev;
            if (ae.Effects.Count > 0) {
                unsupported.Add(at + ": event FX (" + FxNames(ae.Effects) + ")");
            }
            if (ae.Normalize) {
                unsupported.Add(at + ": Normalize");
            }
        }
        foreach (Fade f in new Fade[] { ev.FadeIn, ev.FadeOut }) {
            Effect tr = f.Transition as Effect;
            if (tr != null) {
                unsupported.Add(at + ": transition (" + FxName(tr) + "), imported as a crossfade");
            }
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
            "path", Q(SourcePath(take)),
            "stream", take.StreamIndex.ToString(CultureInfo.InvariantCulture),
            "name", Q(take.Name),
            "fade_in", N(S(ev.FadeIn.Length)),
            "fade_out", N(S(ev.FadeOut.Length)),
            "fade_in_curve", Q(ev.FadeIn.Curve.ToString()),
            "fade_out_curve", Q(ev.FadeOut.Curve.ToString()),
            "gain", N(ev.FadeIn.Gain),
            "mute", B(ev.Mute),
            "locked", B(ev.Locked),
            "group", group,
            "takes", Arr(takes),
            "pan_crop", Arr(panCrop),
            "src_size", srcSize);
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

    // Track Motion other than the default full frame, including 3D motion, shadow and glow.
    static bool HasTrackMotion(VideoTrack vt, Project proj)
    {
        TrackMotion tm = vt.TrackMotion;
        if (tm.MotionKeyframes.Count > 1 || tm.ShadowEnabled || tm.GlowEnabled || vt.CompositeMode == CompositeMode.SrcAlpha3D) {
            return true;
        }
        foreach (TrackMotionKeyframe k in tm.MotionKeyframes) {
            if (k.PositionX != 0 || k.PositionY != 0 || k.PositionZ != 0 || k.RotationX != 0 || k.RotationY != 0 || k.RotationZ != 0
                || k.OrientationX != 0 || k.OrientationY != 0 || k.OrientationZ != 0
                || Math.Abs(k.Width - proj.Video.Width) > 0.5 || Math.Abs(k.Height - proj.Video.Height) > 0.5) {
                return true;
            }
        }
        return false;
    }

    static string V(VideoMotionVertex v)
    {
        return "[" + N(v.X) + "," + N(v.Y) + "]";
    }

    static string FxName(Effect fx)
    {
        return fx == null ? "unknown" : fx.PlugIn != null ? fx.PlugIn.Name : fx.Description;
    }

    static string FxNames(Effects list)
    {
        List<string> names = new List<string>();
        foreach (Effect fx in list) {
            names.Add(FxName(fx));
        }
        return String.Join(", ", names.ToArray());
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
