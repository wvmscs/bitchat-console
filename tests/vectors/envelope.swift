// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

// Test vectors for the BitChat private envelope encryption (tests/dm.rs),
// with the app's own XChaCha20-Poly1305 code. Run from the crate:
//
//   cat ~/Developments_local/bitchat/bitchat/Nostr/XChaCha20Poly1305Compat.swift \
//       tests/vectors/envelope.swift | swift -
//
// Fixed ECDH output (a compressed point, 33 bytes) and nonce: the key
// derivation (NostrProtocol.derivePrivateEnvelopeKey) and the `v2:` layout.
import CryptoKit
import Foundation

func hexData(_ s: String) -> Data {
    var d = Data()
    var i = s.startIndex
    while i < s.endIndex {
        let j = s.index(i, offsetBy: 2)
        d.append(UInt8(s[i..<j], radix: 16)!)
        i = j
    }
    return d
}
func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }

let shared = hexData("02" + String(repeating: "11", count: 32))
let key = HKDF<CryptoKit.SHA256>.deriveKey(
    inputKeyMaterial: SymmetricKey(data: shared), salt: Data(),
    info: Data("nip44-v2".utf8), outputByteCount: 32
).withUnsafeBytes { Data($0) }
print("key \(hex(key))")
let nonce = Data((0..<24).map { UInt8($0) })
let sealed = try XChaCha20Poly1305Compat.seal(plaintext: Data("bitchat1:hello".utf8), key: key, nonce24: nonce)
var combined = nonce
combined.append(sealed.ciphertext)
combined.append(sealed.tag)
let b64 = combined.base64EncodedString()
    .replacingOccurrences(of: "+", with: "-")
    .replacingOccurrences(of: "/", with: "_")
    .replacingOccurrences(of: "=", with: "")
print("v2 v2:\(b64)")
