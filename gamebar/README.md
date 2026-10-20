# Laser Pointer for Xbox Game Bar (prototype)

Games in true exclusive fullscreen own the display, so no ordinary window — the main app's
overlay included — can appear over them. Xbox Game Bar widgets can: Game Bar shows them over
fullscreen games, and Microsoft supports it, so it's safe with anti-cheat. This is the laser as a
widget, to find out how far that goes.

## Try it

1. Press **Win+G** in a game (or anywhere) to open Game Bar.
2. Open the **Widget menu** and choose **Laser Pointer**. Resize the widget to cover the screen.
3. With Game Bar open, hold the left button and draw; right-click clears.
4. **Pin** the widget, close Game Bar (Win+G again), and play: what's pinned stays on top of the
   game. The label in the corner says which mode Game Bar has the widget in.

## Build and install

Needs Visual Studio 2022 with the Universal Windows Platform workload, and Developer Mode on.

```powershell
& "C:\Program Files\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe" `
    gamebar\LaserPointerWidget.csproj /restore /p:Configuration=Release /p:Platform=x64
Add-AppxPackage -Register gamebar\bin\x64\Release\ilc\AppxManifest.xml
```

Remove it from Settings → Apps, or `Get-AppxPackage LaserPointer.GameBarWidget.Dev | Remove-AppxPackage`.

The manifest's Game Bar extension and COM proxy-stub entries come from Microsoft's
[widget samples](https://github.com/microsoft/XboxGameBarSamples); `Laser.cs` ports the main
app's stroke renderer (`src/stroke.rs`).
