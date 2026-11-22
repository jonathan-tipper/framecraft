// An offline rich-text receiver for the macOS-only handoff test. It is never
// shown, activated, or allowed to read the system's general pasteboard.
import AppKit

func verify(_ condition: Bool, _ problem: String) {
    if !condition {
        FileHandle.standardError.write(Data("\(problem)\n".utf8))
        exit(1)
    }
}

verify(CommandLine.arguments.count == 2, "Expected one private pasteboard name.")
let name = NSPasteboard.Name(CommandLine.arguments[1])
verify(name != .general && name != .find && name != .drag && name != .font && name != .ruler,
       "The receiver requires an isolated fixture pasteboard.")
let board = NSPasteboard(name: name)
let application = NSApplication.shared
application.setActivationPolicy(.prohibited)

let receiver = NSTextView(frame: NSRect(x: 0, y: 0, width: 300, height: 80))
receiver.isRichText = true
receiver.importsGraphics = true
receiver.string = "beforeafter"
receiver.setSelectedRange(NSRange(location: 6, length: 0))
verify(receiver.readSelection(from: board), "NSTextView did not accept the captured image.")
verify(receiver.string == "before\u{fffc}after", "The receiver inserted the wrong content or used the wrong caret.")
guard let attachment = receiver.attributedString().attribute(.attachment, at: 6, effectiveRange: nil) as? NSTextAttachment,
      let sourceData = board.data(forType: .png),
      let source = NSImage(data: sourceData),
      let received = attachment.fileWrapper?.regularFileContents.flatMap({ NSImage(data: $0) })
          ?? attachment.image ?? (attachment.attachmentCell as? NSCell)?.image,
      let sourceTIFF = source.tiffRepresentation,
      let receivedTIFF = received.tiffRepresentation,
      let expectedPixels = NSBitmapImageRep(data: sourceTIFF),
      let receivedPixels = NSBitmapImageRep(data: receivedTIFF) else {
    FileHandle.standardError.write(Data("The pasted image attachment could not be decoded.\n".utf8))
    exit(1)
}
verify(expectedPixels.pixelsWide == 7 && expectedPixels.pixelsHigh == 3, "Unexpected fixture dimensions.")
verify(receivedPixels.pixelsWide == 7 && receivedPixels.pixelsHigh == 3, "Incorrect pasted dimensions.")
for y in 0..<3 {
    for x in 0..<7 {
        let expected = expectedPixels.colorAt(x: x, y: y)?.usingColorSpace(.sRGB)
        let received = receivedPixels.colorAt(x: x, y: y)?.usingColorSpace(.sRGB)
        verify(expected != nil && received == expected, "The pasted attachment's pixels differ from the fixture.")
    }
}
verify(application.windows.allSatisfy { !$0.isVisible }, "The receiver displayed an unexpected window.")
print("The private native paste inserted 7 × 3 matching pixels at the original caret.")
