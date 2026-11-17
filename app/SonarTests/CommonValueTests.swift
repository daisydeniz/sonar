import XCTest
@testable import Sonar

final class CommonValueTests: XCTestCase {
    func testAllAgreeGivesValue() {
        let c = CommonValue<String>(["Dm", "Dm", "Dm"])
        XCTAssertEqual(c.value, "Dm")
        XCTAssertFalse(c.isMixed)
    }

    func testDifferentValuesAreMixed() {
        let c = CommonValue<String>(["Dm", "C", "Dm"])
        XCTAssertNil(c.value)
        XCTAssertTrue(c.isMixed)
    }

    func testValueAndMissingAreMixed() {
        XCTAssertTrue(CommonValue<Double>([120, nil]).isMixed)
    }

    func testAllMissingIsEmptyNotMixed() {
        let c = CommonValue<Double>([nil, nil])
        XCTAssertNil(c.value)
        XCTAssertFalse(c.isMixed)
    }

    func testNoRowsAndSingleRow() {
        XCTAssertFalse(CommonValue<String>([]).isMixed)
        XCTAssertEqual(CommonValue<String>(["x"]).value, "x")
    }
}
