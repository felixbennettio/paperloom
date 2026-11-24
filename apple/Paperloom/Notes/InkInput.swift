import SwiftUI

/// A point of a stroke as the pointer gave it: where, in the view, and how
/// hard (none for a mouse or a finger).
struct InkSample {
    var location: CGPoint
    var pressure: Double?
}

/// What is drawing.
enum InkPointer {
    case pen, eraser, finger, mouse, middle
}

/// What a drawing does with pointer input.
struct InkCallbacks {
    var begin: (InkSample, InkPointer) -> Void = { _, _ in }
    var move: ([InkSample]) -> Void = { _ in }
    var end: () -> Void = {}
    /// The stroke was taken over (by a pinch, say) and is dropped.
    var cancel: () -> Void = {}
    var pan: (CGVector) -> Void = { _ in }
    /// By a factor, about a point in the view.
    var zoom: (Double, CGPoint) -> Void = { _, _ in }
}

#if os(iOS)
import UIKit

/// Takes touches over a drawing: an Apple Pencil's force and every point
/// between frames; two fingers pinch to zoom and drag to pan.
final class InkInputView: UIView {
    var callbacks = InkCallbacks()
    private var tracked: UITouch?
    private var pinchScale: CGFloat = 1
    private var panAt: CGPoint = .zero

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
        backgroundColor = .clear
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(pinched(_:)))
        let pan = UIPanGestureRecognizer(target: self, action: #selector(panned(_:)))
        pan.minimumNumberOfTouches = 2
        pan.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)]
        pinch.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)]
        addGestureRecognizer(pinch)
        addGestureRecognizer(pan)
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
    }

    private func sample(_ touch: UITouch) -> InkSample {
        let force = touch.type == .pencil && touch.maximumPossibleForce > 0 ? touch.force / touch.maximumPossibleForce : nil
        return InkSample(location: touch.preciseLocation(in: self), pressure: force.map { Double($0) })
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard tracked == nil, let touch = touches.first else { return }
        tracked = touch
        callbacks.begin(sample(touch), touch.type == .pencil ? .pen : .finger)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = tracked, touches.contains(touch) else { return }
        callbacks.move((event?.coalescedTouches(for: touch) ?? [touch]).map(sample))
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = tracked, touches.contains(touch) else { return }
        tracked = nil
        callbacks.end()
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = tracked, touches.contains(touch) else { return }
        tracked = nil
        callbacks.cancel()
    }

    @objc private func pinched(_ recognizer: UIPinchGestureRecognizer) {
        if recognizer.state == .began { pinchScale = 1 }
        guard recognizer.state == .changed || recognizer.state == .began else { return }
        callbacks.zoom(Double(recognizer.scale / pinchScale), recognizer.location(in: self))
        pinchScale = recognizer.scale
    }

    @objc private func panned(_ recognizer: UIPanGestureRecognizer) {
        let at = recognizer.translation(in: self)
        if recognizer.state == .began { panAt = .zero }
        callbacks.pan(CGVector(dx: at.x - panAt.x, dy: at.y - panAt.y))
        panAt = at
    }
}

struct InkInput: UIViewRepresentable {
    let callbacks: InkCallbacks

    func makeUIView(context: Context) -> InkInputView {
        InkInputView()
    }

    func updateUIView(_ view: InkInputView, context: Context) {
        view.callbacks = callbacks
    }
}
#else
import AppKit

/// Takes the mouse or a tablet pen over a drawing, with the pen's pressure
/// and eraser end; the scroll wheel pans (zooms with ⌘ or Control), a
/// trackpad pinches to zoom, and the middle button drags the page.
final class InkInputView: NSView {
    var callbacks = InkCallbacks()
    private var eraserNear = false
    private var lastDrag: CGPoint?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    private func location(_ event: NSEvent) -> CGPoint {
        convert(event.locationInWindow, from: nil)
    }

    private func sample(_ event: NSEvent) -> InkSample {
        let tablet = event.subtype == .tabletPoint
        return InkSample(location: location(event), pressure: tablet ? Double(event.pressure) : nil)
    }

    override func tabletProximity(with event: NSEvent) {
        eraserNear = event.isEnteringProximity && event.pointingDeviceType == .eraser
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let tablet = event.subtype == .tabletPoint
        callbacks.begin(sample(event), tablet ? (eraserNear ? .eraser : .pen) : .mouse)
    }

    override func mouseDragged(with event: NSEvent) {
        callbacks.move([sample(event)])
    }

    override func mouseUp(with event: NSEvent) {
        callbacks.end()
    }

    override func otherMouseDown(with event: NSEvent) {
        lastDrag = location(event)
    }

    override func otherMouseDragged(with event: NSEvent) {
        let at = location(event)
        if let last = lastDrag { callbacks.pan(CGVector(dx: at.x - last.x, dy: at.y - last.y)) }
        lastDrag = at
    }

    override func otherMouseUp(with event: NSEvent) {
        lastDrag = nil
    }

    override func scrollWheel(with event: NSEvent) {
        if event.modifierFlags.contains(.command) || event.modifierFlags.contains(.control) {
            callbacks.zoom(exp(Double(event.scrollingDeltaY) * 0.01), location(event))
        } else {
            callbacks.pan(CGVector(dx: event.scrollingDeltaX, dy: event.scrollingDeltaY))
        }
    }

    override func magnify(with event: NSEvent) {
        callbacks.zoom(1 + Double(event.magnification), location(event))
    }
}

struct InkInput: NSViewRepresentable {
    let callbacks: InkCallbacks

    func makeNSView(context: Context) -> InkInputView {
        InkInputView()
    }

    func updateNSView(_ view: InkInputView, context: Context) {
        view.callbacks = callbacks
    }
}
#endif
