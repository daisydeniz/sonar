import XCTest
@testable import Sonar

final class CommonTagsTests: XCTestCase {
    func testIntersectionKeepsFirstListOrder() {
        XCTAssertEqual(commonTags([["b", "a", "c"], ["c", "b"], ["b", "c", "x"]]), ["b", "c"])
    }

    func testSingleEmptyAndDisjoint() {
        XCTAssertEqual(commonTags([["a", "b"]]), ["a", "b"])
        XCTAssertEqual(commonTags([]), [])
        XCTAssertEqual(commonTags([["a"], ["b"]]), [])
    }

    func testForceMixed() {
        XCTAssertTrue(CommonValue<String>(["x", "x"], forceMixed: true).isMixed)
    }
}
