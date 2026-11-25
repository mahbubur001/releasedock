import XCTest
@testable import ReleaseDockCore

final class ReleaseNotesTests: XCTestCase {
    func testGroupsListItemsUnderHeadings() {
        let notes = ReleaseNotes(html: """
        <h3>Added</h3>
        <ul><li>Update from the <b>toolbar</b>.</li><li>Settings window with tabs.</li></ul>
        <h3>Fixed</h3>
        <ul><li>No more <code>403</code> rate limit &amp; retries.</li></ul>
        """)
        XCTAssertEqual(notes.sections, [
            .init(title: "Added", items: ["Update from the **toolbar**.", "Settings window with tabs."]),
            .init(title: "Fixed", items: ["No more `403` rate limit & retries."]),
        ])
    }

    func testSingleParagraphBecomesUntitledSection() {
        let notes = ReleaseNotes(html: "<p>Update the SVN working copy and resolve conflicts from Products; go Back.</p>")
        XCTAssertEqual(notes.sections, [.init(title: nil, items: ["Update the SVN working copy and resolve conflicts from Products; go Back."])])
    }

    func testPlainTextAndEmptyInput() {
        XCTAssertEqual(ReleaseNotes(html: "Faster reads.").sections, [.init(title: nil, items: ["Faster reads."])])
        XCTAssertTrue(ReleaseNotes(html: "  ").isEmpty)
        XCTAssertTrue(ReleaseNotes(html: "<h3>Added</h3>").isEmpty, "a heading without items is dropped")
    }

    func testKeepsLinksAndCollapsesWhitespace() {
        let notes = ReleaseNotes(html: "<ul><li>See the\n   <a href=\"https://example.com/log\">changelog</a>.</li></ul>")
        XCTAssertEqual(notes.sections.first?.items, ["See the [changelog](https://example.com/log)."])
    }
}
