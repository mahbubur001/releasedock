import Foundation

/// Release manifest written next to each release's report. It deliberately has no
/// field that could hold a credential.
public struct ReleaseManifest: Codable, Hashable, Sendable {
    public struct File: Codable, Hashable, Sendable {
        public var path: String
        public var size: UInt64
        public var sha256: String
    }

    public var schemaVersion: Int
    public var releaseID: UUID
    public var product: String
    public var slug: String
    public var type: ProductType
    public var version: String
    public var previousVersion: String?
    public var mode: ReleaseMode
    public var sourceKind: SourceKind
    public var sourcePath: String
    public var sourceSHA256: String
    public var candidateDigest: String?
    public var gitCommit: String?
    public var svnURL: String
    public var releasePath: String
    public var svnRevisionBefore: Int?
    public var validation: [ValidationResult]
    public var state: ReleaseState
    public var finalRevision: Int?
    public var files: [File]
    public var generatedAt: Date
    public var generator: String

    public static func make(record: ReleaseRecord, candidate: StagedCandidate?, releasePath: String, gitCommit: String?, generator: String, now: Date = Date()) -> ReleaseManifest {
        ReleaseManifest(
            schemaVersion: 1,
            releaseID: record.id,
            product: record.productName,
            slug: record.productSlug,
            type: record.productType,
            version: record.version ?? "",
            previousVersion: record.previousVersion,
            mode: record.mode,
            sourceKind: record.source.kind,
            sourcePath: record.source.path,
            sourceSHA256: record.sourceSHA256 ?? "",
            candidateDigest: candidate?.treeDigest,
            gitCommit: gitCommit,
            svnURL: record.svnURL,
            releasePath: releasePath,
            svnRevisionBefore: record.revisionBefore,
            validation: record.validation?.results ?? [],
            state: record.state,
            finalRevision: record.committedRevision,
            files: (candidate?.files ?? []).map { File(path: $0.path, size: $0.size, sha256: $0.sha256 ?? "") },
            generatedAt: now,
            generator: generator
        )
    }

    public func encoded() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        encoder.dateEncodingStrategy = .iso8601
        return try encoder.encode(self)
    }
}
