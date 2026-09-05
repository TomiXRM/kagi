// Print the CGWindowID of the frontmost on-screen window whose owning process
// name contains the argument. Used to feed `screencapture -l<id>` so a capture
// contains ONLY that window (no desktop, no other apps).
import CoreGraphics
import Foundation

let needle = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "kagi"
guard
    let infos = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]
else {
    exit(1)
}
for info in infos {
    let owner = info[kCGWindowOwnerName as String] as? String ?? ""
    let layer = info[kCGWindowLayer as String] as? Int ?? -1
    let bounds = info[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let w = bounds["Width"] as? Double ?? 0
    let h = bounds["Height"] as? Double ?? 0
    // layer 0 = normal app window; skip tiny helper windows.
    if owner.lowercased().contains(needle.lowercased()), layer == 0, w > 200, h > 200 {
        if let id = info[kCGWindowNumber as String] as? Int {
            print(id)
            exit(0)
        }
    }
}
exit(2)
