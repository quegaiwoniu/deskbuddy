#!/bin/bash
# 通用 GIF 拆帧器：把 <角色目录>/raw/*.gif 拆成 frames/<动作>/NN.png
# 用法: split_pet.sh <角色目录>
set -euo pipefail
DST="${1:?用法: split_pet.sh <角色目录>}"
RAW="$DST/raw"
[ -d "$RAW" ] || { echo "无 raw/ 目录"; exit 1; }

swift - "$RAW" "$DST/frames" <<'EOF'
import Foundation
import ImageIO
import UniformTypeIdentifiers

let raw = URL(fileURLWithPath: CommandLine.arguments[1])
let dst = URL(fileURLWithPath: CommandLine.arguments[2])
let files = (try? FileManager.default.contentsOfDirectory(at: raw, includingPropertiesForKeys: nil)) ?? []
for file in files where file.pathExtension.lowercased() == "gif" {
    // 动作名 = 文件名去掉 -source-resolution 等后缀
    var action = file.deletingPathExtension().lastPathComponent
    for suffix in ["-source-resolution", "-hd", "-source"] where action.hasSuffix(suffix) {
        action = String(action.dropLast(suffix.count))
    }
    guard let src = CGImageSourceCreateWithURL(file as CFURL, nil) else { continue }
    let count = CGImageSourceGetCount(src)
    let dir = dst.appendingPathComponent(action, isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    for i in 0..<count {
        guard let img = CGImageSourceCreateImageAtIndex(src, i, nil) else { continue }
        let out = dir.appendingPathComponent(String(format: "%02d.png", i))
        guard let d = CGImageDestinationCreateWithURL(out as CFURL, UTType.png.identifier as CFString, 1, nil) else { continue }
        CGImageDestinationAddImage(d, img, nil)
        if CGImageDestinationFinalize(d) { continue }
    }
    print("\(action): \(count) 帧")
}
EOF
echo "拆帧完成 → $DST/frames"
