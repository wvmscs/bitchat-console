// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

// Checks a group state and a group message made by the console with the
// app's own GroupProtocol.swift ($BITCHAT_SRC: a checkout of
// https://github.com/permissionlesstech/bitchat at tag v1.8.0). Run from the crate:
//
//   cargo run -q --example group_vectors | cat tests/vectors/groups_shim.swift \
//     <(grep -v '^import BitFoundation' $BITCHAT_SRC/bitchat/Services/Groups/GroupProtocol.swift) \
//     tests/vectors/groups.swift > /tmp/g.swift && swift /tmp/g.swift
//
// (stdin: state hex, message hex, key hex).
let stateHex = readLine()!, messageHex = readLine()!, keyHex = readLine()!
guard let state = GroupStatePayload.decode(Data(hexString: stateHex)!) else { fatalError("state does not decode") }
print("state: name=\(state.name) epoch=\(state.epoch) members=\(state.members.map { $0.nickname }) creatorSignature=\(state.verifyCreatorSignature())")
guard let env = GroupMessageEnvelope.decode(Data(hexString: messageHex)!) else { fatalError("envelope does not decode") }
let m = try GroupCrypto.openMessage(env, key: Data(hexString: keyHex)!)
print("message: \(m.content) from \(m.senderNickname) id=\(m.messageID) ts=\(m.timestampMs)")
