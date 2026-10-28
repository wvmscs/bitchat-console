// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

// Test vectors for tests/identity.rs, computed with CryptoKit as the BitChat
// app does (NostrIdentityBridge.deriveIdentity(forGeohash:),
// NoiseEncryptionService). Run: swift tests/vectors/identity.swift
import CryptoKit
import Foundation

func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }

// Seed 0x00 0x01 ... 0x1f.
let seed = Data((0..<32).map { UInt8($0) })
for geohash in ["u09tv", "u09tvw", "9q8yy"] {
    for iteration in [UInt32(0), UInt32(1)] {
        var input = Data(geohash.utf8)
        var be = iteration.bigEndian
        withUnsafeBytes(of: &be) { input.append(contentsOf: $0) }
        let code = HMAC<SHA256>.authenticationCode(for: input, using: SymmetricKey(data: seed))
        print("hmac \(geohash) \(iteration) \(hex(Data(code)))")
    }
}

// Noise static key and signing key from fixed raw bytes 0x20 ... 0x3f.
let raw = Data((32..<64).map { UInt8($0) })
let noise = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: raw)
let pub = noise.publicKey.rawRepresentation
print("x25519 \(hex(pub))")
print("fingerprint \(hex(Data(SHA256.hash(data: pub))))")
let signing = try Curve25519.Signing.PrivateKey(rawRepresentation: raw)
print("ed25519 \(hex(signing.publicKey.rawRepresentation))")
