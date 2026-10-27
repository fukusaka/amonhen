import AppKit

/// SSH の接続先の候補: `~/.ssh/config` の `Host`（`*`・`?` のない名前）と `~/.ssh/known_hosts` のホスト（ハッシュに
/// したものは読めないので出ない。ポートがあれば `host:port`）。
enum SSHHosts {
    static func candidates() -> [String] {
        let ssh = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".ssh")
        var names: [String] = []
        if let config = try? String(contentsOf: ssh.appendingPathComponent("config"), encoding: .utf8) {
            for line in config.split(whereSeparator: \.isNewline) {
                let words = line.split(whereSeparator: { $0 == " " || $0 == "\t" || $0 == "=" })
                guard words.count > 1, words[0].lowercased() == "host" else { continue }
                names += words.dropFirst().map(String.init).filter { !$0.contains(where: { "*?!".contains($0) }) }
            }
        }
        if let known = try? String(contentsOf: ssh.appendingPathComponent("known_hosts"), encoding: .utf8) {
            for line in known.split(whereSeparator: \.isNewline) {
                var fields = line.split(separator: " ", omittingEmptySubsequences: true)
                if fields.first?.hasPrefix("@") == true { fields.removeFirst() }  // @cert-authority・@revoked
                guard let hosts = fields.first, !hosts.hasPrefix("#"), !hosts.hasPrefix("|") else { continue }
                for host in hosts.split(separator: ",") {
                    // [host]:port
                    if host.hasPrefix("["), let close = host.firstIndex(of: "]") {
                        let name = host[host.index(after: host.startIndex)..<close]
                        let port = host[close...].dropFirst().drop { $0 == ":" }
                        names.append(port.isEmpty || port == "22" ? String(name) : "\(name):\(port)")
                    } else if !host.contains(where: { "*?!".contains($0) }) {
                        names.append(String(host))
                    }
                }
            }
        }
        // 名前を先に、IP アドレスは後に。
        func isAddress(_ s: String) -> Bool { s.first?.isNumber == true && s.allSatisfy { $0.isNumber || $0 == "." || $0 == ":" } || s.contains("::") }
        var seen = Set<String>()
        return names.filter { seen.insert($0).inserted }
            .sorted { (isAddress($0) ? 1 : 0, $0.lowercased()) < (isAddress($1) ? 1 : 0, $1.lowercased()) }
    }
}
