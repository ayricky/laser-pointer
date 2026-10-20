using System;
using Microsoft.Gaming.XboxGameBar;
using Windows.ApplicationModel;
using Windows.ApplicationModel.Activation;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;

namespace LaserPointerWidget
{
    /// <summary>
    /// Game Bar starts the widget by protocol activation and hands it a window; launched any
    /// other way, the app only explains how to open it.
    /// </summary>
    sealed partial class App : Application
    {
        // Held for as long as the widget lives: it is the connection to Game Bar.
        private XboxGameBarWidget widget;

        public App()
        {
            InitializeComponent();
            Suspending += OnSuspending;
        }

        protected override void OnActivated(IActivatedEventArgs args)
        {
            if (args.Kind != ActivationKind.Protocol
                || !(args is IProtocolActivatedEventArgs protocol)
                || protocol.Uri.Scheme != "ms-gamebarwidget"
                || !(args is XboxGameBarWidgetActivatedEventArgs widgetArgs))
            {
                return;
            }
            // Later activations reuse the widget made the first time.
            if (!widgetArgs.IsLaunchActivation)
            {
                return;
            }
            var frame = new Frame();
            Window.Current.Content = frame;
            widget = new XboxGameBarWidget(widgetArgs, Window.Current.CoreWindow, frame);
            frame.Navigate(typeof(LaserPage), widget);
            Window.Current.Closed += (sender, e) => widget = null;
            Window.Current.Activate();
        }

        protected override void OnLaunched(LaunchActivatedEventArgs e)
        {
            if (!(Window.Current.Content is Frame frame))
            {
                frame = new Frame();
                Window.Current.Content = frame;
            }
            if (frame.Content == null)
            {
                frame.Navigate(typeof(InfoPage));
            }
            Window.Current.Activate();
        }

        // The app is never suspended while Game Bar holds a widget, so this is the widget's end.
        private void OnSuspending(object sender, SuspendingEventArgs e)
        {
            var deferral = e.SuspendingOperation.GetDeferral();
            widget = null;
            deferral.Complete();
        }
    }
}
