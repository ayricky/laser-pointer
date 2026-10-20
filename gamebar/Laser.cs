using System;
using System.Collections.Generic;
using System.Numerics;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Geometry;
using Windows.UI;

namespace LaserPointerWidget
{
    /// <summary>A sample of the pointer, with when it was taken in milliseconds.</summary>
    struct LaserPoint
    {
        public Vector2 At;
        public double Time;
    }

    /// <summary>How the laser looks; the defaults are the app's Laser preset.</summary>
    sealed class Look
    {
        public Color Color = Color.FromArgb(255, 0xff, 0x2a, 0x2a);
        public float Width = 4;
        public float Opacity = 1;
        public double FadeMs = 1000;
        public float Trail = 500;
        public int Smoothing = 2;
    }

    sealed class Stroke
    {
        public readonly List<LaserPoint> Points = new List<LaserPoint>();
    }

    /// <summary>
    /// The app's stroke renderer (src/stroke.rs), ported: a stroke tapers to nothing behind its
    /// head by age and by distance, is smoothed and curved through its samples, and is filled as
    /// one rounded outline per glow layer.
    /// </summary>
    static class Laser
    {
        // (width multiplier, opacity, mix towards white) for each pass, widest first: the Glow style.
        static readonly (float Width, float Alpha, float White)[] Layers =
        {
            (3.4f, 0.10f, 0f), (2.1f, 0.20f, 0f), (1f, 1f, 0f),
        };

        const float MinStep = 2, CurveStep = 3;
        const int CapSteps = 8;

        public static float MinimumStep => MinStep;

        static float EaseOut(float k) => 1 - (float)Math.Pow(1 - k, 4);

        /// <summary>Whether every point has aged out.</summary>
        public static bool Expired(Stroke stroke, double now, Look look) =>
            stroke.Points.Count == 0 || now - stroke.Points[stroke.Points.Count - 1].Time >= look.FadeMs;

        public static void Draw(CanvasDrawingSession session, Stroke stroke, double now, Look look)
        {
            var points = stroke.Points;
            int n = points.Count;
            if (n == 0)
            {
                return;
            }
            float baseRadius = look.Width / 2;
            var radii = new float[n];
            float behind = 0;
            for (int i = n - 1; i >= 0; i--)
            {
                if (i + 1 < n)
                {
                    behind += Vector2.Distance(points[i].At, points[i + 1].At);
                }
                float age = 1 - (float)((now - points[i].Time) / look.FadeMs);
                float length = 1 - behind / look.Trail;
                radii[i] = baseRadius * Math.Min(EaseOut(Math.Max(0, length)), EaseOut(Math.Max(0, age)));
            }
            // Keep one faded point so the tail still tapers all the way to nothing.
            int faded = 0;
            while (faded < n && radii[faded] < 0.1f)
            {
                faded++;
            }
            int drop = faded == n ? n : Math.Max(0, faded - 1);
            if (drop > 0)
            {
                points.RemoveRange(0, drop);
                var kept = new float[n - drop];
                Array.Copy(radii, drop, kept, 0, kept.Length);
                radii = kept;
                n -= drop;
            }
            if (n == 0)
            {
                return;
            }
            var path = Smooth(points, look.Smoothing);
            CurveThrough(path, radii, out var centre, out var widths);
            foreach (var layer in Layers)
            {
                var outline = Outline(centre, widths, layer.Width);
                if (outline.Length < 3)
                {
                    continue;
                }
                byte Mix(byte channel) => (byte)(channel + (255 - channel) * layer.White);
                var color = Color.FromArgb(
                    (byte)(255 * layer.Alpha * look.Opacity),
                    Mix(look.Color.R),
                    Mix(look.Color.G),
                    Mix(look.Color.B));
                using (var geometry = CanvasGeometry.CreatePolygon(session, outline))
                {
                    session.FillGeometry(geometry, color);
                }
            }
        }

        /// <summary>Symmetric window that shrinks to nothing at both ends, so the head stays put.</summary>
        static Vector2[] Smooth(List<LaserPoint> points, int span)
        {
            int n = points.Count;
            var output = new Vector2[n];
            for (int i = 0; i < n; i++)
            {
                int s = Math.Min(span, Math.Min(i, n - 1 - i));
                Vector2 sum = Vector2.Zero;
                float weights = 0;
                for (int offset = -s; offset <= s; offset++)
                {
                    float weight = s + 1 - Math.Abs(offset);
                    sum += points[i + offset].At * weight;
                    weights += weight;
                }
                output[i] = sum / weights;
            }
            return output;
        }

        /// <summary>
        /// Quadratic curves midpoint to midpoint with each sample as the control, finely enough
        /// that sparse samples from a fast flick still come out round.
        /// </summary>
        static void CurveThrough(Vector2[] path, float[] radii, out List<Vector2> points, out List<float> widths)
        {
            points = new List<Vector2>();
            widths = new List<float>();
            int count = path.Length;
            if (count < 3)
            {
                points.AddRange(path);
                widths.AddRange(radii);
                return;
            }
            Vector2 Mid(int i) => (path[i] + path[i + 1]) / 2;
            float MidRadius(int i) => (radii[i] + radii[i + 1]) / 2;
            points.Add(path[0]);
            points.Add(Mid(0));
            widths.Add(radii[0]);
            widths.Add(MidRadius(0));
            for (int i = 1; i < count - 1; i++)
            {
                Vector2 from = Mid(i - 1), control = path[i], to = Mid(i);
                float r0 = MidRadius(i - 1), r1 = radii[i], r2 = MidRadius(i);
                float length = Vector2.Distance(from, control) + Vector2.Distance(control, to);
                int steps = Math.Min(16, Math.Max(1, (int)Math.Ceiling(length / CurveStep)));
                for (int k = 1; k <= steps; k++)
                {
                    float t = (float)k / steps;
                    float a = (1 - t) * (1 - t), b = 2 * (1 - t) * t, c = t * t;
                    points.Add(a * from + b * control + c * to);
                    widths.Add(a * r0 + b * r1 + c * r2);
                }
            }
            points.Add(path[count - 1]);
            widths.Add(radii[count - 1]);
        }

        /// <summary>Up one side, round the head, back down the other side and round the tail.</summary>
        static Vector2[] Outline(List<Vector2> centre, List<float> widths, float scale)
        {
            int n = centre.Count;
            var tangents = new Vector2[n];
            var direction = new Vector2(1, 0);
            for (int i = 0; i < n; i++)
            {
                var a = centre[Math.Max(0, i - 1)];
                var b = centre[Math.Min(n - 1, i + 1)];
                float length = Vector2.Distance(a, b);
                if (length > 1e-3f)
                {
                    direction = (b - a) / length;
                }
                tangents[i] = direction;
            }
            Vector2 Offset(int i, float along, float across)
            {
                var t = tangents[i];
                float r = widths[i] * scale;
                return centre[i] + new Vector2(t.X * along - t.Y * across, t.Y * along + t.X * across) * r;
            }
            var outline = new List<Vector2>(n * 2 + CapSteps * 2);
            for (int i = 0; i < n; i++)
            {
                outline.Add(Offset(i, 0, 1));
            }
            for (int s = 1; s < CapSteps; s++)
            {
                double angle = Math.PI * s / CapSteps;
                outline.Add(Offset(n - 1, (float)Math.Sin(angle), (float)Math.Cos(angle)));
            }
            for (int i = n - 1; i >= 0; i--)
            {
                outline.Add(Offset(i, 0, -1));
            }
            for (int s = 1; s < CapSteps; s++)
            {
                double angle = Math.PI * s / CapSteps;
                outline.Add(Offset(0, -(float)Math.Sin(angle), -(float)Math.Cos(angle)));
            }
            return outline.ToArray();
        }
    }
}
