using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Numerics;
using Microsoft.Gaming.XboxGameBar;
using Microsoft.Graphics.Canvas.Brushes;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Windows.UI;
using Windows.UI.Core;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Input;
using Windows.UI.Xaml.Media;
using Windows.UI.Xaml.Navigation;

namespace LaserPointerWidget
{
    /// <summary>
    /// The widget: a transparent surface the laser is drawn on. Game Bar gives it the mouse while
    /// Game Bar is open; pinned, it stays on top of the game with input going to the game.
    /// </summary>
    public sealed partial class LaserPage : Page
    {
        private readonly Stopwatch clock = Stopwatch.StartNew();
        private readonly List<Stroke> strokes = new List<Stroke>();
        private readonly Look look = new Look();
        private Stroke drawing;
        private uint? pointerId;
        private Vector2? pointer;
        private bool rendering;
        private XboxGameBarWidget widget;
        private CoreCursor savedCursor;

        public LaserPage()
        {
            InitializeComponent();
        }

        private double Now => clock.Elapsed.TotalMilliseconds;

        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            widget = e.Parameter as XboxGameBarWidget;
            if (widget != null)
            {
                widget.GameBarDisplayModeChanged += (sender, args) => OnUi(ShowStatus);
                widget.PinnedChanged += (sender, args) => OnUi(ShowStatus);
            }
            ShowStatus();
        }

        private async void OnUi(Action action) =>
            await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () => action());

        private void ShowStatus()
        {
            if (widget == null)
            {
                Status.Text = "Not running inside Game Bar";
                return;
            }
            bool open = widget.GameBarDisplayMode == XboxGameBarDisplayMode.Foreground;
            Status.Text = open
                ? "Game Bar open: hold the left button to draw, right-click clears"
                : widget.Pinned
                    ? "Pinned: the laser stays on top of the game"
                    : "Game Bar closed";
        }

        // ---- Drawing loop: runs only while something on screen is changing --------------------

        private void Wake()
        {
            if (!rendering)
            {
                rendering = true;
                CompositionTarget.Rendering += OnRendering;
            }
        }

        private void OnRendering(object sender, object e)
        {
            Surface.Invalidate();
            double now = Now;
            strokes.RemoveAll(stroke => stroke != drawing && Laser.Expired(stroke, now, look));
            if (strokes.Count == 0 && drawing == null)
            {
                rendering = false;
                CompositionTarget.Rendering -= OnRendering;
            }
        }

        private void Surface_Draw(CanvasControl sender, CanvasDrawEventArgs args)
        {
            double now = Now;
            foreach (var stroke in strokes)
            {
                Laser.Draw(args.DrawingSession, stroke, now, look);
            }
            if (pointer is Vector2 at)
            {
                DrawCursor(sender, args, at);
            }
        }

        /// <summary>The soft glow cursor: a bloom, a core in the laser colour and a white-hot centre.</summary>
        private void DrawCursor(CanvasControl sender, CanvasDrawEventArgs args, Vector2 at)
        {
            var c = look.Color;
            var stops = new[]
            {
                new CanvasGradientStop { Position = 0, Color = Color.FromArgb(190, c.R, c.G, c.B) },
                new CanvasGradientStop { Position = 0.3f, Color = Color.FromArgb(82, c.R, c.G, c.B) },
                new CanvasGradientStop { Position = 1, Color = Color.FromArgb(0, c.R, c.G, c.B) },
            };
            using (var bloom = new CanvasRadialGradientBrush(sender, stops) { Center = at, RadiusX = 20, RadiusY = 20 })
            {
                args.DrawingSession.FillCircle(at, 20, bloom);
            }
            args.DrawingSession.FillCircle(at, 3, c);
            args.DrawingSession.FillCircle(at, 1.3f, Color.FromArgb(235, 255, 255, 255));
        }

        // ---- Input ----------------------------------------------------------------------------------

        private void Surface_PointerEntered(object sender, PointerRoutedEventArgs e)
        {
            // The laser is the cursor here.
            savedCursor = Window.Current.CoreWindow.PointerCursor;
            Window.Current.CoreWindow.PointerCursor = null;
        }

        private void Surface_PointerExited(object sender, PointerRoutedEventArgs e)
        {
            if (savedCursor != null)
            {
                Window.Current.CoreWindow.PointerCursor = savedCursor;
            }
            pointer = null;
            Surface.Invalidate();
        }

        private void Surface_PointerPressed(object sender, PointerRoutedEventArgs e)
        {
            var point = e.GetCurrentPoint(Surface);
            if (point.Properties.IsRightButtonPressed)
            {
                strokes.Clear();
                drawing = null;
                Surface.Invalidate();
                return;
            }
            if (!point.Properties.IsLeftButtonPressed || pointerId != null)
            {
                return;
            }
            pointerId = e.Pointer.PointerId;
            Surface.CapturePointer(e.Pointer);
            drawing = new Stroke();
            drawing.Points.Add(new LaserPoint { At = point.Position.ToVector2(), Time = Now });
            strokes.Add(drawing);
            pointer = point.Position.ToVector2();
            Wake();
        }

        private void Surface_PointerMoved(object sender, PointerRoutedEventArgs e)
        {
            pointer = e.GetCurrentPoint(Surface).Position.ToVector2();
            if (drawing != null && e.Pointer.PointerId == pointerId)
            {
                // Every sample since the last event, oldest first, so fast flicks stay round.
                var samples = e.GetIntermediatePoints(Surface);
                for (int i = samples.Count - 1; i >= 0; i--)
                {
                    Append(samples[i].Position.ToVector2());
                }
            }
            Wake();
        }

        private void Append(Vector2 at)
        {
            var points = drawing.Points;
            if (points.Count > 0 && Vector2.Distance(points[points.Count - 1].At, at) < Laser.MinimumStep)
            {
                return;
            }
            points.Add(new LaserPoint { At = at, Time = Now });
        }

        private void Surface_PointerReleased(object sender, PointerRoutedEventArgs e)
        {
            if (e.Pointer.PointerId != pointerId)
            {
                return;
            }
            pointerId = null;
            drawing = null;
            Surface.ReleasePointerCapture(e.Pointer);
            Wake();
        }
    }
}
