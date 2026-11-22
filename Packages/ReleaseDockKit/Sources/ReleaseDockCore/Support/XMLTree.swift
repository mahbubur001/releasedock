import Foundation

/// Minimal XML DOM for parsing `svn --xml` output.
public final class XMLElement: @unchecked Sendable {
    public let name: String
    public let attributes: [String: String]
    public fileprivate(set) var children: [XMLElement] = []
    public fileprivate(set) var text: String = ""

    init(name: String, attributes: [String: String]) {
        self.name = name
        self.attributes = attributes
    }

    public func child(_ name: String) -> XMLElement? { children.first { $0.name == name } }
    public func all(_ name: String) -> [XMLElement] { children.filter { $0.name == name } }

    /// Depth-first search for descendants with `name`.
    public func descendants(_ name: String) -> [XMLElement] {
        var result: [XMLElement] = []
        for c in children {
            if c.name == name { result.append(c) }
            result += c.descendants(name)
        }
        return result
    }

    public var trimmedText: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    public static func parse(_ data: Data) throws -> XMLElement {
        let builder = Builder()
        let parser = XMLParser(data: data)
        parser.shouldResolveExternalEntities = false
        parser.delegate = builder
        guard parser.parse(), let root = builder.root else {
            throw parser.parserError ?? CocoaError(.coderReadCorrupt)
        }
        return root
    }

    private final class Builder: NSObject, XMLParserDelegate {
        var root: XMLElement?
        var stack: [XMLElement] = []

        func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes attributeDict: [String: String] = [:]) {
            let element = XMLElement(name: elementName, attributes: attributeDict)
            if let parent = stack.last { parent.children.append(element) } else { root = element }
            stack.append(element)
        }

        func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) {
            stack.removeLast()
        }

        func parser(_ parser: XMLParser, foundCharacters string: String) {
            stack.last?.text += string
        }
    }
}
