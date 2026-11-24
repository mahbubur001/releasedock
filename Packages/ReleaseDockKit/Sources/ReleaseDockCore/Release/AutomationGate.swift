import Foundation

/// Decides whether a release may publish without a human confirmation. Every
/// condition must hold; the returned list explains each one that does not.
public enum AutomationGate {
    public static func blockingReasons(
        product: Product,
        record: ReleaseRecord,
        currentPreviewFingerprint: String?
    ) -> [String] {
        var reasons: [String] = []
        if product.releaseMode != .automatic || !product.automationEnabled {
            reasons.append("Automatic publishing is not explicitly enabled for \(product.name)")
        }
        guard let report = record.validation else {
            return reasons + ["The release has not been validated"]
        }
        func results(_ check: ValidationCheckID) -> [ValidationResult] { report.results.filter { $0.check == check } }
        func allPassed(_ check: ValidationCheckID) -> Bool {
            let r = results(check)
            return !r.isEmpty && r.allSatisfy { $0.status == .passed }
        }

        if !allPassed(.versionValidity) { reasons.append("The version is not a valid, final release version") }
        switch record.kind {
        case .version:
            if !allPassed(.versionIncrease) { reasons.append("The version is not confirmed to be newer than the current release") }
        case .readmeUpdate:
            if !allPassed(.releasedVersion) { reasons.append("The package is not confirmed to be the released version") }
        }
        if !allPassed(.productIdentity) { reasons.append("Product identity is not fully confirmed") }
        if report.isBlocking { reasons.append("\(report.blockingResults.count) required check(s) did not pass") }
        let skippedRequired = report.results.filter { $0.status == .skipped && $0.check.isRequired }
        if !skippedRequired.isEmpty {
            reasons.append("Required checks were skipped: \(skippedRequired.map(\.name).joined(separator: ", "))")
        }

        if let repo = record.repository {
            if !repo.workingCopyChanges.isEmpty { reasons.append("The working copy had local changes") }
            if !repo.outOfDatePaths.isEmpty { reasons.append("The working copy is not current with the repository") }
            switch record.kind {
            case .version:
                if repo.releasePathExists != false { reasons.append("The release tag is not confirmed absent") }
            case .readmeUpdate:
                if repo.releasePathExists != true { reasons.append("The released tag is not confirmed to exist") }
            }
            if repo.authentication != .keychain && repo.authentication != .cachedBySVN { reasons.append("No valid authentication is available") }
        } else {
            reasons.append("Repository state is unknown")
        }

        if let preview = record.preview {
            if currentPreviewFingerprint != preview.fingerprint { reasons.append("The release preview changed") }
        } else {
            reasons.append("No release preview exists")
        }
        if !record.errors.isEmpty { reasons.append("The release has unresolved errors") }
        return reasons
    }
}
