import Foundation

/// 環境変数の表で値を伏せる変数（名前で決める。値の中身は見ない）。名前を英数字以外で語に区切り、
/// どれかの語が次に当たれば伏せる。
/// - TOKEN・SECRET・PASSWORD・PASSWD・CREDENTIAL・PRIVATE を含む
/// - AUTH か PASS そのもの（ASKPASS や AUTHOR は当たらない）
/// - KEY で終わる（KEY・APIKEY・ACCESSKEY。KEYID や KEYMAP は当たらない）
/// ただし、秘密ではないとわかっているもの（SSH の agent の場所、Amon Hen の渡す変数の一覧）は出す。
public enum SecretNames {
    static let contained = ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "PRIVATE"]
    static let whole: Set<String> = ["AUTH", "PASS"]
    static let shown: Set<String> = ["SSH_AUTH_SOCK", "AMONHEN_PASS_ENV"]

    public static func isSecret(_ name: String) -> Bool {
        let upper = name.uppercased()
        guard !shown.contains(upper) else { return false }
        return upper.split(whereSeparator: { !($0.isLetter || $0.isNumber) }).contains { word in
            contained.contains(where: word.contains) || whole.contains(String(word)) || word.hasSuffix("KEY")
        }
    }
}
