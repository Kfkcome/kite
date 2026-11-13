# tern patch to gpui-pre-macos 0.3.8

Upstream `start_window_move` calls `performWindowDragWithEvent:`. On macOS 27 that
WindowServer gesture can stick: content clicks never reach the view, the traffic-light
buttons still do, and the main thread waits in `nextEvent`. tern's UI task is the only
SSH consumer, so a stuck gesture also stops remote output.

On macOS 27 and later, a titlebar press records the mouse and the window origin.
`mouseDragged:` applies the delta with `setFrameOrigin:`. The opaque-rect override is
unchanged, so AppKit still does not drag the titlebar itself or delay titlebar clicks.
Earlier systems keep `performWindowDragWithEvent:`.
