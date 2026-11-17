import Foundation

/// What a field shows for several selected rows: the shared value when all
/// rows agree, `mixed` when they differ (display only, no product logic).
struct CommonValue<T: Equatable>: Equatable {
    /// The shared value (nil when unknown for all rows, or when mixed).
    let value: T?
    let isMixed: Bool

    /// `forceMixed`: the selection includes rows that are not loaded, so
    /// agreement cannot be known.
    init(_ values: [T?], forceMixed: Bool = false) {
        if forceMixed {
            value = nil
            isMixed = true
            return
        }
        guard let first = values.first else {
            value = nil
            isMixed = false
            return
        }
        if values.dropFirst().allSatisfy({ $0 == first }) {
            value = first
            isMixed = false
        } else {
            value = nil
            isMixed = true
        }
    }
}
