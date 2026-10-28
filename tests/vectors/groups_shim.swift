// Stand-ins for the BitFoundation helpers GroupProtocol.swift uses.
import CryptoKit
import Foundation

extension Data {
    init?(hexString: String) {
        var d = Data()
        var i = hexString.startIndex
        while i < hexString.endIndex {
            guard let j = hexString.index(i, offsetBy: 2, limitedBy: hexString.endIndex),
                  let b = UInt8(hexString[i..<j], radix: 16) else { return nil }
            d.append(b)
            i = j
        }
        self = d
    }
    func hexEncodedString() -> String { map { String(format: "%02x", $0) }.joined() }
    func sha256Hash() -> Data { Data(SHA256.hash(data: self)) }
}
struct PeerID { init(groupID: Data) {} }
