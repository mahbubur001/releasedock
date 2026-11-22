import Foundation

/// A faithful port of PHP's `version_compare()` (php-src `ext/standard/versioning.c`).
///
/// WordPress core and WordPress.org compare plugin and theme versions with
/// `version_compare()`, so ReleaseDock uses identical semantics rather than SemVer:
///
/// - `-`, `_`, `+` and other non-alphanumerics become `.`
/// - a `.` is inserted between digit/non-digit transitions (`1.0rc1` → `1.0.rc.1`)
/// - special forms order: any unknown string < `dev` < `alpha`=`a` < `beta`=`b`
///   < `RC`=`rc` < number < `pl`=`p`
public enum PHPVersion {
    /// Returns -1, 0 or 1 exactly like `version_compare($a, $b)`.
    public static func compare(_ a: String, _ b: String) -> Int {
        versionCompare(Array(a.utf8), Array(b.utf8))
    }

    /// The canonical form PHP compares (`php_canonicalize_version`).
    public static func canonicalize(_ version: String) -> String {
        String(decoding: canonicalize(Array(version.utf8)), as: UTF8.self)
    }

    // MARK: - Port

    private static let dot = UInt8(ascii: ".")

    private static func isDigit(_ c: UInt8) -> Bool { c >= 0x30 && c <= 0x39 }
    private static func isAlnum(_ c: UInt8) -> Bool {
        isDigit(c) || (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A)
    }
    private static func isdig(_ c: UInt8) -> Bool { isDigit(c) && c != dot }
    private static func isndig(_ c: UInt8) -> Bool { !isDigit(c) && c != dot }
    private static func isSpecialVer(_ c: UInt8) -> Bool {
        c == UInt8(ascii: "-") || c == UInt8(ascii: "_") || c == UInt8(ascii: "+")
    }

    static func canonicalize(_ version: [UInt8]) -> [UInt8] {
        guard let first = version.first else { return [] }
        var q: [UInt8] = [first]
        var lp = first
        for p in version.dropFirst() {
            let lq = q.last!
            if isSpecialVer(p) {
                if lq != dot { q.append(dot) }
            } else if (isndig(lp) && isdig(p)) || (isdig(lp) && isndig(p)) {
                if lq != dot { q.append(dot) }
                q.append(p)
            } else if !isAlnum(p) {
                if lq != dot { q.append(dot) }
            } else {
                q.append(p)
            }
            lp = p
        }
        if q.last == dot { q.removeLast() }
        return q
    }

    private static let specialForms: [(name: [UInt8], order: Int)] = [
        (Array("dev".utf8), 0),
        (Array("alpha".utf8), 1),
        (Array("a".utf8), 1),
        (Array("beta".utf8), 2),
        (Array("b".utf8), 2),
        (Array("RC".utf8), 3),
        (Array("rc".utf8), 3),
        (Array("#".utf8), 4),
        (Array("pl".utf8), 5),
        (Array("p".utf8), 5),
    ]

    /// `strncmp(form, name, strlen(name)) == 0` — i.e. `form` starts with `name`.
    private static func specialOrder(_ form: ArraySlice<UInt8>) -> Int {
        for special in specialForms where form.starts(with: special.name) {
            return special.order
        }
        return -1
    }

    private static func normalize(_ value: Int) -> Int { value > 0 ? 1 : (value < 0 ? -1 : 0) }

    private static func compareSpecial(_ a: ArraySlice<UInt8>, _ b: ArraySlice<UInt8>) -> Int {
        normalize(specialOrder(a) - specialOrder(b))
    }

    private static func compareNumeric(_ a: ArraySlice<UInt8>, _ b: ArraySlice<UInt8>) -> Int {
        let n1 = a.drop(while: { $0 == UInt8(ascii: "0") }).prefix(while: isDigit)
        let n2 = b.drop(while: { $0 == UInt8(ascii: "0") }).prefix(while: isDigit)
        if n1.count != n2.count { return n1.count < n2.count ? -1 : 1 }
        for (x, y) in zip(n1, n2) where x != y { return x < y ? -1 : 1 }
        return 0
    }

    private static let numberMarker = ArraySlice(Array("#N#".utf8))

    static func versionCompare(_ orig1: [UInt8], _ orig2: [UInt8]) -> Int {
        if orig1.isEmpty || orig2.isEmpty {
            if orig1.isEmpty && orig2.isEmpty { return 0 }
            return orig1.isEmpty ? -1 : 1
        }
        let v1 = orig1[0] == UInt8(ascii: "#") ? orig1 : canonicalize(orig1)
        let v2 = orig2[0] == UInt8(ascii: "#") ? orig2 : canonicalize(orig2)

        // Emulates the C loop: p = start of current component, n = index of the next '.' (or nil).
        var p1 = 0, p2 = 0
        var n1: Int? = 0, n2: Int? = 0
        var compare = 0

        func component(_ v: [UInt8], from p: Int, to n: Int?) -> ArraySlice<UInt8> {
            v[p..<(n ?? v.count)]
        }

        while p1 < v1.count, p2 < v2.count, n1 != nil, n2 != nil {
            n1 = v1[p1...].firstIndex(of: dot)
            n2 = v2[p2...].firstIndex(of: dot)
            let c1 = component(v1, from: p1, to: n1)
            let c2 = component(v2, from: p2, to: n2)
            let d1 = c1.first.map(isDigit) ?? false
            let d2 = c2.first.map(isDigit) ?? false
            if d1 && d2 {
                compare = compareNumeric(c1, c2)
            } else if !d1 && !d2 {
                compare = compareSpecial(c1, c2)
            } else if d1 {
                compare = compareSpecial(numberMarker, c2)
            } else {
                compare = compareSpecial(c1, numberMarker)
            }
            if compare != 0 { break }
            if let n = n1 { p1 = n + 1 }
            if let n = n2 { p2 = n + 1 }
        }

        if compare == 0 {
            if n1 != nil {
                let rest = Array(v1[min(p1, v1.count)...])
                if let f = rest.first, isDigit(f) {
                    compare = 1
                } else {
                    compare = versionCompare(rest, Array(numberMarker))
                }
            } else if n2 != nil {
                let rest = Array(v2[min(p2, v2.count)...])
                if let f = rest.first, isDigit(f) {
                    compare = -1
                } else {
                    compare = versionCompare(Array(numberMarker), rest)
                }
            }
        }
        return compare
    }
}
