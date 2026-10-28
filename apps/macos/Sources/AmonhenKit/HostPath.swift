import Foundation

/// git を動かす機械（SSH 先）のパス。その機械のエンジンが返す形で、POSIX は `/a/b`、Windows は `C:\a\b`・
/// `\\server\share\a`（区切りは `\`。打ち込んだ `/` も区切りとして読む）。どちらかは根の形で見分ける。
public enum HostPath {
    /// Windows のパスか（ドライブ `C:` か UNC `\\` で始まる）。
    public static func isWindows(_ path: String) -> Bool {
        drive(path) != nil || path.hasPrefix(#"\\"#)
    }

    /// 根: `/`、`C:\`、`\\server\share\`。根で始まらなければ（`~/x`・相対パス）nil。
    public static func root(of path: String) -> String? {
        if path.hasPrefix("/") { return "/" }
        if let drive = drive(path) {
            let rest = path.dropFirst(2)
            return rest.isEmpty || rest.first == "\\" || rest.first == "/" ? drive + "\\" : nil
        }
        if path.hasPrefix(#"\\"#) {
            let parts = path.dropFirst(2).split(whereSeparator: { $0 == "\\" || $0 == "/" })
            guard parts.count >= 2 else { return nil }
            return #"\\"# + parts[0] + "\\" + parts[1] + "\\"
        }
        return nil
    }

    public static func isAbsolute(_ path: String) -> Bool {
        root(of: path) != nil
    }

    /// 上のフォルダを根から: `/a/b/c` なら `/`・`/a`・`/a/b`、`C:\a\b` なら `C:\`・`C:\a`。根なら空。
    public static func ancestors(of path: String) -> [String] {
        guard let top = root(of: path) else { return [] }
        let parts = components(String(path.dropFirst(min(top.count, path.count))), windows: isWindows(top))
        guard !parts.isEmpty else { return [] }
        var out = [top]
        for part in parts.dropLast() {
            out.append(join(out[out.count - 1], String(part)))
        }
        return out
    }

    /// フォルダの中の名前のパス（区切りはフォルダの形に合わせる）。
    public static func join(_ folder: String, _ name: String) -> String {
        let windows = isWindows(folder)
        if folder.hasSuffix("/") || (windows && folder.hasSuffix("\\")) { return folder + name }
        return folder + (windows ? "\\" : "/") + name
    }

    /// 最後の名前（根ならそのまま）。
    public static func name(_ path: String) -> String {
        components(path, windows: isWindows(path)).last.map(String.init) ?? path
    }

    /// 打ち込んだパスを、上のフォルダと名前に分ける（`~/a/b` なら `~/a` と `b`、`/git` なら `/` と `git`）。
    /// `windows` なら `\` も区切り（`~\bin\git.exe` は根で見分けられない）。分けられなければ nil。
    public static func split(_ path: String, windows: Bool) -> (parent: String, name: String)? {
        let windows = windows || isWindows(path)
        let isSeparator = { (c: Character) in c == "/" || (windows && c == "\\") }
        var path = Substring(path)
        let top = root(of: String(path))
        while path.count > (top?.count ?? 0), let last = path.last, isSeparator(last) { path = path.dropLast() }
        guard let slash = path.lastIndex(where: isSeparator) else { return nil }
        let name = String(path[path.index(after: slash)...])
        let parent = String(path[...slash])
        guard !name.isEmpty else { return nil }
        if let top, parent.count <= top.count { return (top, name) }
        return (String(parent.dropLast()), name)
    }

    /// ホームの下なら `~/…`（Places・`ssh://` の場所と同じ書き方。Windows でも `/` で続ける。エンジンはどちらも読む）。
    public static func shown(_ path: String, home: String?) -> String {
        guard let home, !home.isEmpty, root(of: home) != home else { return path }
        let windows = isWindows(home)
        // Windows は大文字小文字を区別しない。
        let same = { (a: Substring, b: String) in
            windows ? a.caseInsensitiveCompare(b) == .orderedSame : a == b
        }
        let trimmed = home.hasSuffix("/") || (windows && home.hasSuffix("\\")) ? String(home.dropLast()) : home
        guard path.count >= trimmed.count, same(path.prefix(trimmed.count), trimmed) else { return path }
        let rest = path.dropFirst(trimmed.count)
        if rest.isEmpty { return "~" }
        guard rest.first == "/" || (windows && rest.first == "\\") else { return path }
        let parts = components(String(rest), windows: windows)
        return parts.isEmpty ? "~" : "~/" + parts.joined(separator: "/")
    }

    /// `C:` のドライブ。
    private static func drive(_ path: String) -> String? {
        let chars = Array(path.prefix(2))
        guard chars.count == 2, chars[1] == ":", chars[0].isASCII, chars[0].isLetter else { return nil }
        return String(chars)
    }

    private static func components(_ path: String, windows: Bool) -> [Substring] {
        path.split(whereSeparator: { $0 == "/" || (windows && $0 == "\\") })
    }
}
