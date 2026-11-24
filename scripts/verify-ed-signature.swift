// Verifies a Sparkle EdDSA signature against a public key, as installed copies will.
// Usage: swift scripts/verify-ed-signature.swift <archive> <base64-signature> <base64-public-key>
import CryptoKit
import Foundation

let args = CommandLine.arguments
guard args.count == 4,
      let archive = FileManager.default.contents(atPath: args[1]),
      let signature = Data(base64Encoded: args[2]),
      let keyData = Data(base64Encoded: args[3]),
      let key = try? Curve25519.Signing.PublicKey(rawRepresentation: keyData)
else {
    FileHandle.standardError.write(Data("usage: verify-ed-signature <archive> <signature> <public-key>\n".utf8))
    exit(2)
}
exit(key.isValidSignature(signature, for: archive) ? 0 : 1)
