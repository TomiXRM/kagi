// Post mouse and keyboard events to one macOS process without moving the system cursor.
//
// swiftc scripts/pidclick.swift -o /tmp/pidclick
// swiftc scripts/pidcursor.swift -o /tmp/pidcursor   # the cursor, next to pidclick
// /tmp/pidclick windows --pid <pid>
// /tmp/pidclick --pid <pid> --window-id <id> move <x> <y>
// /tmp/pidclick --pid <pid> --window-id <id> click <x> <y>
// /tmp/pidclick --pid <pid> --window-id <id> rclick <x> <y>
// /tmp/pidclick --pid <pid> --window-id <id> scroll <x> <y> <lines>
// /tmp/pidclick --pid <pid> --window-id <id> key <keycode> [cmd] [shift] [alt] [ctrl]
// /tmp/pidclick --pid <pid> --window-id <id> type <text>
//
// Coordinates are logical points relative to the target window's top-left corner.
// The responsible process needs macOS Accessibility permission.
//
// `move`, `click`, `rclick` and `scroll` also show where they act (#948): they
// tell the pidcursor daemon for this PID, starting it from pidclick's own
// directory when it is not running, and wait for its cursor to glide there
// before posting. `--no-cursor` or `PIDCLICK_CURSOR=0` turns this off.
import Carbon
import CoreGraphics
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data(("pidclick: \(message)\n").utf8))
    exit(2)
}

func warn(_ message: String) {
    FileHandle.standardError.write(Data(("pidclick: \(message)\n").utf8))
}

func usage() -> Never {
    fail("usage: pidclick windows --pid <pid> | pidclick --pid <pid> --window-id <id> [--no-cursor] move|click|rclick <x> <y> | scroll <x> <y> <lines> | key <keycode> [cmd|shift|alt|ctrl ...] | type <text>")
}

func printUsage() {
    print("usage: pidclick windows --pid <pid> | pidclick --pid <pid> --window-id <id> [--no-cursor] move|click|rclick <x> <y> | scroll <x> <y> <lines> | key <keycode> [cmd|shift|alt|ctrl ...] | type <text>")
}

// MARK: - The cursor daemon's socket (keep in step with pidcursor.swift)

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

/// pidcursor's glide time: the cursor arrives before the event does.
let cursorGlideMicroseconds: useconds_t = 300_000

/// One datagram to the daemon; false when nothing is listening.
func sendToCursor(_ message: String, pid: pid_t) -> Bool {
    let fd = socket(AF_UNIX, SOCK_DGRAM, 0)
    guard fd >= 0 else { return false }
    defer { close(fd) }
    let bytes = Array(message.utf8)
    return withSocketAddress(cursorSocketPath(for: pid)) { sendto(fd, bytes, bytes.count, 0, $0, $1) } == bytes.count
}

/// Start `pidcursor --pid <pid>` detached (its own session, no terminal), from
/// the directory pidclick runs from.
func startCursor(pid: pid_t) -> Bool {
    let directory = (URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath().path as NSString)
        .deletingLastPathComponent
    let daemon = (directory as NSString).appendingPathComponent("pidcursor")
    guard FileManager.default.isExecutableFile(atPath: daemon) else {
        warn("no cursor: \(daemon) is not built (swiftc scripts/pidcursor.swift -o \(daemon)); --no-cursor silences this")
        return false
    }
    var attributes: posix_spawnattr_t?
    posix_spawnattr_init(&attributes)
    defer { posix_spawnattr_destroy(&attributes) }
    posix_spawnattr_setflags(&attributes, Int16(POSIX_SPAWN_SETSID))
    var files: posix_spawn_file_actions_t?
    posix_spawn_file_actions_init(&files)
    defer { posix_spawn_file_actions_destroy(&files) }
    posix_spawn_file_actions_addopen(&files, 0, "/dev/null", O_RDONLY, 0)
    posix_spawn_file_actions_addopen(&files, 1, "/dev/null", O_WRONLY, 0)
    posix_spawn_file_actions_addopen(&files, 2, "/dev/null", O_WRONLY, 0)
    let argv: [UnsafeMutablePointer<CChar>?] = [strdup(daemon), strdup("--pid"), strdup(String(pid)), nil]
    defer { argv.forEach { free($0) } }
    var child: pid_t = 0
    guard posix_spawn(&child, daemon, &files, &attributes, argv, environ) == 0 else {
        warn("no cursor: could not start \(daemon)")
        return false
    }
    return true
}

/// Show the agent cursor at `x`,`y` (window-relative) and wait for it to get
/// there. Never fails the action: without a cursor the event is posted anyway.
func showCursor(_ operation: String, pid: pid_t, window: CGWindowID, x: Double, y: Double, extra: String = "") {
    let message = "\(operation) \(window) \(x) \(y)\(extra.isEmpty ? "" : " \(extra)")"
    var delivered = sendToCursor(message, pid: pid)
    if !delivered, startCursor(pid: pid) {
        for _ in 0..<50 where !delivered {
            usleep(20_000)
            delivered = sendToCursor(message, pid: pid)
        }
    }
    if delivered { usleep(cursorGlideMicroseconds) }
}

func parsePID(_ value: String) -> pid_t? {
    guard let parsed = Int32(value), parsed > 0 else { return nil }
    return pid_t(parsed)
}

func parseWindowID(_ value: String) -> CGWindowID? {
    guard let parsed = UInt32(value), parsed != 0 else { return nil }
    return CGWindowID(parsed)
}

func number(_ value: Any?) -> CGFloat? {
    guard let value = value as? NSNumber else { return nil }
    let result = CGFloat(value.doubleValue)
    return result.isFinite ? result : nil
}

struct TargetWindow {
    let id: CGWindowID
    let bounds: CGRect
}

func targetWindow(id: CGWindowID, pid: pid_t) -> TargetWindow? {
    guard let list = CGWindowListCopyWindowInfo([.optionIncludingWindow], id) as? [[String: Any]],
          let info = list.first,
          let ownerPID = (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value,
          ownerPID == pid,
          let dictionary = info[kCGWindowBounds as String] as? [String: Any],
          let x = number(dictionary["X"]),
          let y = number(dictionary["Y"]),
          let width = number(dictionary["Width"]),
          let height = number(dictionary["Height"]),
          width > 0,
          height > 0
    else {
        return nil
    }
    return TargetWindow(id: id, bounds: CGRect(x: x, y: y, width: width, height: height))
}

func listWindows(pid: pid_t) {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
    for window in list {
        guard (window[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid,
              let id = (window[kCGWindowNumber as String] as? NSNumber)?.uint32Value,
              let dictionary = window[kCGWindowBounds as String] as? [String: Any],
              let x = number(dictionary["X"]),
              let y = number(dictionary["Y"]),
              let width = number(dictionary["Width"]),
              let height = number(dictionary["Height"])
        else { continue }
        let title = window[kCGWindowName as String] as? String ?? ""
        let layer = (window[kCGWindowLayer as String] as? NSNumber)?.intValue ?? -1
        print("\(id)\tlayer=\(layer)\t\(Int(x)),\(Int(y)) \(Int(width))x\(Int(height))\t\(title)")
    }
}

var arguments = Array(CommandLine.arguments.dropFirst())
var pid: pid_t?
var windowID: CGWindowID?
var command: [String] = []
var cursorWanted = ProcessInfo.processInfo.environment["PIDCLICK_CURSOR"] != "0"
if arguments.count == 1, arguments[0] == "--help" || arguments[0] == "-h" {
    printUsage()
    exit(0)
}
var index = 0
while index < arguments.count {
    if command.first == "type" {
        command.append(arguments[index])
        index += 1
        continue
    }
    switch arguments[index] {
    case "--pid":
        guard index + 1 < arguments.count, pid == nil,
              let parsed = parsePID(arguments[index + 1]) else { usage() }
        pid = parsed
        index += 2
    case "--window-id":
        guard index + 1 < arguments.count, windowID == nil,
              let parsed = parseWindowID(arguments[index + 1]) else { usage() }
        windowID = parsed
        index += 2
    case "--no-cursor" where command.isEmpty:
        cursorWanted = false
        index += 1
    default:
        command.append(arguments[index])
        index += 1
    }
}

guard let pid, let operation = command.first else { usage() }
if operation == "windows" {
    guard command.count == 1, windowID == nil else { usage() }
    listWindows(pid: pid)
    exit(0)
}
guard let windowID, let target = targetWindow(id: windowID, pid: pid) else {
    fail("window does not exist, is not usable, or does not belong to pid \(pid)")
}

let source = CGEventSource(stateID: .hidSystemState)
guard let source else { fail("could not create event source") }
let clickGroup = Int64(DispatchTime.now().uptimeNanoseconds % 1_000_000_000)

func post(_ event: CGEvent) {
    // 51 is the AppKit target window number. 40 directs the event to the process,
    // 58 groups a click sequence, and 91/92 preserve the window-under-pointer fields.
    func field(_ rawValue: UInt32, _ value: Int64) {
        event.setIntegerValueField(unsafeBitCast(rawValue, to: CGEventField.self), value: value)
    }
    field(51, Int64(target.id))
    field(40, Int64(pid))
    field(58, clickGroup)
    field(91, Int64(target.id))
    field(92, Int64(target.id))
    event.postToPid(pid)
    usleep(20_000)
}

func point(from values: ArraySlice<String>) -> CGPoint {
    guard values.count == 2,
          let x = Double(values[values.startIndex]),
          let y = Double(values[values.index(after: values.startIndex)]),
          x.isFinite,
          y.isFinite,
          x >= 0,
          y >= 0,
          x <= target.bounds.width,
          y <= target.bounds.height
    else { fail("coordinates must be finite points inside the target window") }
    return CGPoint(x: target.bounds.minX + x, y: target.bounds.minY + y)
}

func mouse(_ type: CGEventType, at point: CGPoint, button: CGMouseButton) -> CGEvent {
    guard let event = CGEvent(
        mouseEventSource: source,
        mouseType: type,
        mouseCursorPosition: point,
        mouseButton: button
    ) else { fail("could not create mouse event") }
    event.setIntegerValueField(.mouseEventClickState, value: 1)
    return event
}

func sendKey(code: UInt16, shifted: Bool = false, modifiers: CGEventFlags = []) {
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down) else {
            fail("could not create keyboard event")
        }
        event.flags = shifted ? modifiers.union(.maskShift) : modifiers
        post(event)
    }
}

func currentLayoutMap() -> [Character: (code: UInt16, shifted: Bool)]? {
    let inputSource = TISCopyCurrentKeyboardLayoutInputSource().takeRetainedValue()
    guard let layoutData = TISGetInputSourceProperty(inputSource, kTISPropertyUnicodeKeyLayoutData) else {
        return nil
    }
    let data = Unmanaged<CFData>.fromOpaque(layoutData).takeUnretainedValue() as Data
    var map: [Character: (code: UInt16, shifted: Bool)] = [:]
    data.withUnsafeBytes { bytes in
        guard let baseAddress = bytes.baseAddress else { return }
        let layout = baseAddress.assumingMemoryBound(to: UCKeyboardLayout.self)
        for shifted in [false, true] {
            let modifiers = shifted ? (UInt32(shiftKey) >> 8) & 0xFF : 0
            for code in UInt16(0)..<128 {
                var deadKeyState: UInt32 = 0
                var length = 0
                var characters = [UniChar](repeating: 0, count: 4)
                let result = UCKeyTranslate(
                    layout, code, UInt16(kUCKeyActionDown), modifiers,
                    UInt32(LMGetKbdType()), UInt32(kUCKeyTranslateNoDeadKeysMask),
                    &deadKeyState, characters.count, &length, &characters
                )
                guard result == noErr, length == 1,
                      let character = String(utf16CodeUnits: characters, count: 1).first,
                      map[character] == nil
                else { continue }
                map[character] = (code, shifted)
            }
        }
    }
    return map
}

let values = command.dropFirst()

/// Show the agent cursor at a global `location` inside the target window.
func cursor(_ operation: String, at location: CGPoint, extra: String = "") {
    guard cursorWanted else { return }
    showCursor(
        operation, pid: pid, window: target.id,
        x: location.x - target.bounds.minX, y: location.y - target.bounds.minY, extra: extra
    )
}

switch operation {
case "move":
    let location = point(from: values)
    cursor("move", at: location)
    post(mouse(.mouseMoved, at: location, button: .left))
case "click", "rclick":
    let location = point(from: values)
    let button: CGMouseButton = operation == "rclick" ? .right : .left
    cursor(operation, at: location)
    post(mouse(.mouseMoved, at: location, button: .left)) // GPUI needs hover before a click.
    post(mouse(button == .right ? .rightMouseDown : .leftMouseDown, at: location, button: button))
    post(mouse(button == .right ? .rightMouseUp : .leftMouseUp, at: location, button: button))
case "scroll":
    // A list long enough to scroll is the only place some per-tab state shows
    // up at all (#737). Negative lines scroll down, matching a trackpad's
    // natural direction.
    guard values.count == 3, let lines = Int32(values[values.index(values.startIndex, offsetBy: 2)])
    else { fail("scroll takes <x> <y> <lines>") }
    let location = point(from: values.prefix(2))
    cursor("scroll", at: location, extra: String(lines))
    post(mouse(.mouseMoved, at: location, button: .left)) // the list under the pointer scrolls.
    guard let wheel = CGEvent(
        scrollWheelEvent2Source: source,
        units: .line,
        wheelCount: 1,
        wheel1: lines,
        wheel2: 0,
        wheel3: 0
    ) else { fail("could not create a scroll event") }
    wheel.location = location
    post(wheel)
case "key":
    guard let first = values.first, let code = UInt16(first), code < 128 else { usage() }
    var modifiers: CGEventFlags = []
    for modifier in values.dropFirst() {
        switch modifier {
        case "cmd": modifiers.insert(.maskCommand)
        case "shift": modifiers.insert(.maskShift)
        case "alt": modifiers.insert(.maskAlternate)
        case "ctrl": modifiers.insert(.maskControl)
        default: fail("unsupported modifier \(modifier)")
        }
    }
    sendKey(code: code, modifiers: modifiers)
case "type":
    let text = values.joined(separator: " ")
    guard !text.isEmpty else { fail("text is empty") }
    guard !text.unicodeScalars.contains(where: { $0 == "\n" || $0 == "\r" || $0 == "\t" }) else {
        fail("type does not support newline or tab characters")
    }
    guard let map = currentLayoutMap() else { fail("could not read the current keyboard layout") }
    var keys: [(code: UInt16, shifted: Bool)] = []
    for character in text {
        guard let key = map[character] else {
            fail("current keyboard layout cannot type \(String(character).debugDescription)")
        }
        keys.append(key)
    }
    for key in keys { sendKey(code: key.code, shifted: key.shifted) }
default:
    fail("unknown command \(operation)")
}
