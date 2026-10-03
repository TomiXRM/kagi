// Draw an agent cursor where scripts/pidclick.swift acts, just above the one
// window it drives, so a person watching a Tier B run can follow it (#948).
//
// swiftc scripts/pidcursor.swift -o /tmp/pidcursor
//
// pidclick starts this daemon itself (from the directory pidclick runs from)
// the first time a `move` / `click` / `rclick` / `scroll` cannot reach it, one
// daemon per target PID:
//
//   /tmp/pidcursor --pid <target pid>
//
// It listens on a Unix datagram socket, `<per-user temp dir>/pidcursor/<pid>.sock`,
// for one-line messages: `<op> <window id> <x> <y> [lines]`, coordinates in
// logical points from the window's top-left corner, as pidclick takes them.
//
// It never takes the foreground: an accessory app that never activates, one
// click-through, non-activating panel the size of the target window, ordered
// just above that window. It comes to the very front only when the target
// window already is the frontmost window, so it never covers another app's
// window. It ignores windows of any other process.
//
// It exits when the target process exits, or after 10 minutes without a
// message (PIDCURSOR_IDLE_EXIT overrides the seconds). After 15 seconds
// without a message the cursor fades out over 180 ms, and comes back with the
// next one.
//
// The ordering rule (front only when the target is the frontmost visible
// window) and the idle / fade timings follow trycua/cua's cua-driver
// (libs/cua-driver/rust/crates/platform-macos/src/cursor/overlay.rs and
// cua-driver-core/src/agent_cursor.rs); no code is copied from it.
// Copyright (c) 2025 Cua AI, Inc., MIT License.
import AppKit
import Darwin

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data(("pidcursor: \(message)\n").utf8))
    exit(2)
}

// MARK: - Socket (keep in step with pidclick.swift)

func cursorSocketDirectory() -> String {
    var buffer = [CChar](repeating: 0, count: Int(PATH_MAX))
    let length = confstr(_CS_DARWIN_USER_TEMP_DIR, &buffer, buffer.count)
    let temp = length > 0 ? String(cString: buffer) : NSTemporaryDirectory()
    return (temp as NSString).appendingPathComponent("pidcursor")
}

func cursorSocketPath(for pid: pid_t) -> String {
    (cursorSocketDirectory() as NSString).appendingPathComponent("\(pid).sock")
}

func withSocketAddress<T>(_ path: String, _ body: (UnsafePointer<sockaddr>, socklen_t) -> T) -> T? {
    var address = sockaddr_un()
    address.sun_family = sa_family_t(AF_UNIX)
    let bytes = Array(path.utf8)
    guard bytes.count < MemoryLayout.size(ofValue: address.sun_path) else { return nil }
    withUnsafeMutableBytes(of: &address.sun_path) { raw in
        raw.copyBytes(from: bytes)
        raw[bytes.count] = 0
    }
    address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
    return withUnsafePointer(to: &address) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            body($0, socklen_t(MemoryLayout<sockaddr_un>.size))
        }
    }
}

/// How long the cursor glides to a new point. pidclick waits this long before
/// it posts the event, so the cursor arrives before the click does.
let glideSeconds = 0.28

// MARK: - Arguments

let targetPID: pid_t = {
    let arguments = Array(CommandLine.arguments.dropFirst())
    guard arguments.count == 2, arguments[0] == "--pid", let parsed = Int32(arguments[1]), parsed > 0
    else { fail("usage: pidcursor --pid <target pid>") }
    return parsed
}()
if kill(targetPID, 0) != 0 { exit(0) }

let idleExitSeconds = ProcessInfo.processInfo.environment["PIDCURSOR_IDLE_EXIT"]
    .flatMap(Double.init).flatMap { $0 > 0 ? $0 : nil } ?? 600
let idleFadeSeconds = 15.0
let fadeSeconds = 0.18

// MARK: - The socket this daemon owns

let socketPath = cursorSocketPath(for: targetPID)

func ownSocket() -> Int32 {
    let directory = cursorSocketDirectory()
    if mkdir(directory, 0o700) != 0 && errno != EEXIST { fail("cannot create \(directory)") }
    var info = stat()
    guard lstat(directory, &info) == 0, (info.st_mode & S_IFMT) == S_IFDIR, info.st_uid == getuid()
    else { fail("\(directory) is not this user's directory") }
    // Another daemon already serves this target: leave it be.
    let probe = socket(AF_UNIX, SOCK_DGRAM, 0)
    let ping = Array("ping".utf8)
    let alive = withSocketAddress(socketPath) { sendto(probe, ping, ping.count, 0, $0, $1) } == ping.count
    close(probe)
    if alive { exit(0) }
    unlink(socketPath)
    let fd = socket(AF_UNIX, SOCK_DGRAM, 0)
    guard fd >= 0, withSocketAddress(socketPath, { bind(fd, $0, $1) }) == 0 else {
        fail("cannot bind \(socketPath)")
    }
    _ = fcntl(fd, F_SETFL, O_NONBLOCK)
    return fd
}

/// A normal end: the socket and pidclick's log of this daemon go with it.
func finish() -> Never {
    unlink(socketPath)
    unlink((cursorSocketDirectory() as NSString).appendingPathComponent("\(targetPID).log"))
    exit(0)
}

// MARK: - The target window, read through the public window list

struct TargetWindow {
    let frame: CGRect // global, top-left origin, points
    let onScreen: Bool
}

func targetWindow(_ id: CGWindowID) -> TargetWindow? {
    guard let info = (CGWindowListCopyWindowInfo([.optionIncludingWindow], id) as? [[String: Any]])?.first,
          (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == targetPID,
          let bounds = info[kCGWindowBounds as String] as? [String: Any],
          let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary),
          rect.width > 0, rect.height > 0
    else { return nil }
    let onScreen = (info[kCGWindowIsOnscreen as String] as? NSNumber)?.boolValue ?? false
    return TargetWindow(frame: rect, onScreen: onScreen)
}

/// The target window is the frontmost app's topmost normal window.
func targetIsFrontmost(_ id: CGWindowID) -> Bool {
    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == targetPID,
          let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
            as? [[String: Any]]
    else { return false }
    let top = list.first { window in
        guard (window[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == targetPID,
              (window[kCGWindowLayer as String] as? NSNumber)?.intValue == 0,
              let bounds = window[kCGWindowBounds as String] as? [String: Any],
              let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary)
        else { return false }
        return rect.width > 1 && rect.height > 1
    }
    return (top?[kCGWindowNumber as String] as? NSNumber)?.uint32Value == id
}

/// Global top-left coordinates to AppKit's bottom-left screen coordinates.
func appKitFrame(_ rect: CGRect) -> NSRect {
    let primaryHeight = NSScreen.screens.first?.frame.height ?? rect.maxY
    return NSRect(x: rect.minX, y: primaryHeight - rect.maxY, width: rect.width, height: rect.height)
}

// MARK: - Drawing

let accent = NSColor(srgbRed: 0.49, green: 0.36, blue: 1.0, alpha: 1)
let secondary = NSColor(srgbRed: 1.0, green: 0.58, blue: 0.18, alpha: 1)

struct Glide {
    let from: CGPoint
    let to: CGPoint
    let start: CFTimeInterval
}

struct Pulse {
    let at: CGPoint
    let color: NSColor
    let start: CFTimeInterval
}

struct ScrollMark {
    let at: CGPoint
    let down: Bool
    let start: CFTimeInterval
}

final class CursorView: NSView {
    var position: CGPoint?
    var glide: Glide?
    var pulse: Pulse?
    var scrollMark: ScrollMark?

    override var isFlipped: Bool { true }

    var animating: Bool {
        let now = CACurrentMediaTime()
        return glide != nil
            || pulse.map { now - $0.start < 0.3 } == true
            || scrollMark.map { now - $0.start < 0.45 } == true
    }

    /// Advance the glide; a point the glide has reached starts its pulse.
    func step(_ now: CFTimeInterval) {
        guard let glide else { return }
        let progress = min(1, (now - glide.start) / glideSeconds)
        let eased = progress < 0.5 ? 4 * pow(progress, 3) : 1 - pow(-2 * progress + 2, 3) / 2
        let dx = glide.to.x - glide.from.x
        let dy = glide.to.y - glide.from.y
        // A gentle arc rather than a straight line, bowing to one side.
        let distance = hypot(dx, dy)
        let normal = distance > 0 ? CGPoint(x: -dy / distance, y: dx / distance) : .zero
        let bow = distance * 0.12
        let c1 = CGPoint(x: glide.from.x + dx * 0.3 + normal.x * bow, y: glide.from.y + dy * 0.3 + normal.y * bow)
        let c2 = CGPoint(x: glide.from.x + dx * 0.7 + normal.x * bow, y: glide.from.y + dy * 0.7 + normal.y * bow)
        let t = CGFloat(eased)
        let u = 1 - t
        position = CGPoint(
            x: u * u * u * glide.from.x + 3 * u * u * t * c1.x + 3 * u * t * t * c2.x + t * t * t * glide.to.x,
            y: u * u * u * glide.from.y + 3 * u * u * t * c1.y + 3 * u * t * t * c2.y + t * t * t * glide.to.y
        )
        if progress >= 1 { self.glide = nil }
    }

    override func draw(_ dirtyRect: NSRect) {
        let now = CACurrentMediaTime()
        if let pulse, glide == nil {
            let progress = CGFloat(min(1, (now - pulse.start) / 0.3))
            let radius = 5 + 17 * progress
            let ring = NSBezierPath(ovalIn: NSRect(
                x: pulse.at.x - radius, y: pulse.at.y - radius, width: radius * 2, height: radius * 2))
            ring.lineWidth = 3
            pulse.color.withAlphaComponent(0.9 * (1 - progress)).setStroke()
            ring.stroke()
        }
        if let mark = scrollMark, glide == nil {
            let progress = CGFloat(min(1, (now - mark.start) / 0.45))
            let direction: CGFloat = mark.down ? 1 : -1
            accent.withAlphaComponent(0.9 * (1 - progress)).setStroke()
            for step in 0..<2 {
                let y = mark.at.y + direction * (26 + CGFloat(step) * 7 + progress * 6)
                let chevron = NSBezierPath()
                chevron.move(to: CGPoint(x: mark.at.x - 6, y: y - direction * 4))
                chevron.line(to: CGPoint(x: mark.at.x, y: y))
                chevron.line(to: CGPoint(x: mark.at.x + 6, y: y - direction * 4))
                chevron.lineWidth = 2.5
                chevron.lineCapStyle = .round
                chevron.lineJoinStyle = .round
                chevron.stroke()
            }
        }
        guard let position else { return }
        // An arrow pointer with its tip on the point acted on.
        let arrow = NSBezierPath()
        let points: [(CGFloat, CGFloat)] = [(0, 0), (0, 18), (4.6, 13.8), (7.8, 21), (10.6, 19.8), (7.4, 12.8), (13.2, 12.8)]
        arrow.move(to: CGPoint(x: position.x + points[0].0, y: position.y + points[0].1))
        for point in points.dropFirst() {
            arrow.line(to: CGPoint(x: position.x + point.0, y: position.y + point.1))
        }
        arrow.close()
        arrow.lineJoinStyle = .round
        NSGraphicsContext.saveGraphicsState()
        let shadow = NSShadow()
        shadow.shadowColor = NSColor.black.withAlphaComponent(0.35)
        shadow.shadowBlurRadius = 3
        shadow.shadowOffset = NSSize(width: 0, height: -1)
        shadow.set()
        accent.setFill()
        arrow.fill()
        NSGraphicsContext.restoreGraphicsState()
        NSColor.white.setStroke()
        arrow.lineWidth = 1.5
        arrow.stroke()
    }
}

// MARK: - The panel above the target window

final class Overlay {
    let panel: NSPanel
    let view = CursorView()
    var windowID: CGWindowID?
    var lastMessage = CACurrentMediaTime()
    var frameTimer: Timer?
    var ticks = 0
    var faded = false

    init() {
        panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 10, height: 10),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.ignoresMouseEvents = true
        panel.level = .normal
        panel.hidesOnDeactivate = false
        panel.isReleasedWhenClosed = false
        panel.becomesKeyOnlyIfNeeded = true
        // On every Space, like cua-driver's overlay: a panel tied to the Space
        // it was created on could not follow the target window to another
        // Space (#949 review). Where the target is not on screen, `pin()`
        // orders the panel out, so joining every Space never shows it over
        // other windows.
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .fullScreenAuxiliary, .ignoresCycle]
        panel.contentView = view
        Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in self?.housekeeping() }
    }

    /// Follow the target window: its frame, whether it is on screen, and the
    /// place just above it. Off screen (another Space, minimized, closed), the
    /// panel is ordered out so it never shows over anything else.
    func pin() {
        guard let windowID, let target = targetWindow(windowID), target.onScreen else {
            panel.orderOut(nil)
            return
        }
        let frame = appKitFrame(target.frame)
        if panel.frame != frame { panel.setFrame(frame, display: false) }
        panel.order(.above, relativeTo: Int(windowID))
        if targetIsFrontmost(windowID) { panel.orderFrontRegardless() }
    }

    func handle(_ message: String) {
        let parts = message.split(separator: " ").map(String.init)
        guard parts.count >= 4,
              ["move", "click", "rclick", "scroll"].contains(parts[0]),
              let id = UInt32(parts[1]),
              let x = Double(parts[2]), let y = Double(parts[3]), x.isFinite, y.isFinite,
              targetWindow(id) != nil // only the target process's windows
        else { return }
        lastMessage = CACurrentMediaTime()
        if faded {
            faded = false
            panel.alphaValue = 1
        }
        if windowID != id {
            windowID = id
            view.position = nil
        }
        pin()
        let point = CGPoint(x: x, y: y)
        let from = view.position ?? CGPoint(x: view.bounds.midX, y: view.bounds.midY)
        let now = CACurrentMediaTime()
        view.glide = Glide(from: from, to: point, start: now)
        switch parts[0] {
        case "click": view.pulse = Pulse(at: point, color: accent, start: now + glideSeconds)
        case "rclick": view.pulse = Pulse(at: point, color: secondary, start: now + glideSeconds)
        case "scroll":
            let lines = parts.count > 4 ? Int(parts[4]) ?? 0 : 0
            view.scrollMark = ScrollMark(at: point, down: lines < 0, start: now + glideSeconds)
        default: break
        }
        animate()
    }

    func animate() {
        guard frameTimer == nil else { return }
        frameTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak self] timer in
            guard let self else { return timer.invalidate() }
            self.view.step(CACurrentMediaTime())
            self.view.needsDisplay = true
            if !self.view.animating {
                timer.invalidate()
                self.frameTimer = nil
            }
        }
    }

    func housekeeping() {
        let idle = CACurrentMediaTime() - lastMessage
        if idle >= idleExitSeconds { finish() }
        guard windowID != nil else { return }
        ticks += 1
        if !faded && idle >= idleFadeSeconds {
            faded = true
            NSAnimationContext.runAnimationGroup { context in
                context.duration = fadeSeconds
                panel.animator().alphaValue = 0
            }
        }
        // Follow moves and resizes; re-assert the order about once a second.
        if let windowID, let target = targetWindow(windowID), target.onScreen {
            let frame = appKitFrame(target.frame)
            if panel.frame != frame || ticks % 5 == 0 || !panel.isVisible { pin() }
        } else if panel.isVisible {
            panel.orderOut(nil)
        }
    }
}

// MARK: - Run

let socketFD = ownSocket()
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let overlay = Overlay()

let exitWatch = DispatchSource.makeProcessSource(identifier: targetPID, eventMask: .exit, queue: .main)
exitWatch.setEventHandler { finish() }
exitWatch.resume()
// The target may have exited before the watch started.
if kill(targetPID, 0) != 0 { finish() }

var signalSources: [DispatchSourceSignal] = []
for signalNumber in [SIGTERM, SIGINT, SIGHUP] {
    signal(signalNumber, SIG_IGN)
    let source = DispatchSource.makeSignalSource(signal: signalNumber, queue: .main)
    source.setEventHandler { finish() }
    source.resume()
    signalSources.append(source)
}

let reader = DispatchSource.makeReadSource(fileDescriptor: socketFD, queue: .main)
reader.setEventHandler {
    var buffer = [UInt8](repeating: 0, count: 512)
    while true {
        let count = recv(socketFD, &buffer, buffer.count, 0)
        guard count > 0 else { break }
        if let message = String(bytes: buffer[0..<count], encoding: .utf8) {
            overlay.handle(message.trimmingCharacters(in: .whitespacesAndNewlines))
        }
    }
}
reader.resume()
app.run()
