import Foundation
import SwiftUI

/// Sizes follow the compact, two-action radial design. The cursor sits at the ring's center.
enum RingLayout {
  // The extra canvas space keeps shadows and the optional passphrase controls unclipped.
  static let canvasWidth: CGFloat = 640
  static let canvasHeight: CGFloat = 760
  static let centerY: CGFloat = 264
  static let outerRadius: CGFloat = 148
  static let innerRadius: CGFloat = 73
  static let hubDiameter: CGFloat = 132
  static let labelRadius: CGFloat = 111
  static let hoverLift: CGFloat = 5
  // Six percent enlargement gives labels emphasis without changing their hit geometry.
  static let labelHoverScale: CGFloat = 1.06
  static let dividerGap: Double = 0.8
  // The second layer sits outside Seal with an 18-point corridor between the two rings.
  static let branchInnerRadius: CGFloat = 166
  static let branchOuterRadius: CGFloat = 236
  static let branchLabelRadius: CGFloat = 201
  // Each child covers 66 degrees on the left; the horizontal seam leaves room between labels.
  static let branchStartAngle: Double = 110
  static let branchDividerGap: Double = 4
  // Reserve hover movement around the largest arc so gestures never move the ring's frame.
  static let interactionDiameter: CGFloat = (branchOuterRadius + hoverLift) * 2
  // Keep controls one padding step beyond the active ring's hover extent.
  static let compactAccessoryY: CGFloat = centerY + outerRadius + hoverLift + padding
  static let expandedAccessoryY: CGFloat = centerY + branchOuterRadius + hoverLift + padding
  static let optionsWidth: CGFloat = 302
  // Keep expanded Other explanations and hidden-mode fields inside the overlay; excess content scrolls.
  static func accessoryY(isSealExpanded: Bool) -> CGFloat {
    isSealExpanded ? expandedAccessoryY : compactAccessoryY
  }

  static func maximumAccessoryHeight(isSealExpanded: Bool) -> CGFloat {
    canvasHeight - accessoryY(isSealExpanded: isSealExpanded) - padding
  }
  // Sending steps live in a bounded popover rather than enlarging the radial menu.
  static let guideWidth: CGFloat = 340
  static let guideHeight: CGFloat = 360
  static let guideTextSize: CGFloat = 12
  // The selected image mode uses a subtle blue wash over the shared glass.
  static let selectedModeOpacity: Double = 0.16
  static let closeY: CGFloat = 82
  static let closeDiameter: CGFloat = 24
  // Small offsets are from the shared four-point visual spacing scale.
  static let hairline: CGFloat = 0.75
  static let smallGap: CGFloat = 4
  static let gap: CGFloat = 8
  static let padding: CGFloat = 12
  static let fieldHeight: CGFloat = 34
  static let capsuleRadius: CGFloat = 17
  static let hubPadding: CGFloat = 16
  static let iconSize: CGFloat = 26
  static let titleSize: CGFloat = 13
  static let captionSize: CGFloat = 10
  static let tinySize: CGFloat = 9
  static let shadowRadius: CGFloat = 22
  static let shadowY: CGFloat = 12
  // Hidden content contracts toward the pointer rather than appearing as a window.
  static let collapsedScale: CGFloat = 0.82
  static let collapsedRotation: Double = -8
  static let collapsedBlur: CGFloat = 7

  static var canvasSize: CGSize { CGSize(width: canvasWidth, height: canvasHeight) }
  static var center: CGPoint { CGPoint(x: canvasWidth / 2, y: centerY) }
  static var diameter: CGFloat { outerRadius * 2 }

  static func frame(around cursor: CGPoint, inside bounds: CGRect, accessoryHeight: CGFloat)
    -> CGRect
  {
    // Fit the visible ring and controls. Transparent canvas margins may extend offscreen.
    let leftMargin = branchOuterRadius + hoverLift + shadowRadius
    let rightMargin = outerRadius + hoverLift + shadowRadius
    let topMargin = max(branchOuterRadius + shadowRadius, centerY - closeY + closeDiameter / 2)
    // Reserve expansion space once, so opening the children never shifts the ring under the cursor.
    let expandedHeight = min(accessoryHeight, maximumAccessoryHeight(isSealExpanded: true))
    let bottomMargin = max(
      outerRadius + shadowRadius, expandedAccessoryY - centerY + expandedHeight)
    let adjustedX = fitted(
      cursor.x, minimum: bounds.minX + leftMargin, maximum: bounds.maxX - rightMargin)
    let adjustedY = fitted(
      cursor.y, minimum: bounds.minY + bottomMargin, maximum: bounds.maxY - topMargin)
    return CGRect(
      x: adjustedX - center.x, y: adjustedY - (canvasHeight - center.y),
      width: canvasWidth, height: canvasHeight)
  }

  private static func fitted(_ value: CGFloat, minimum: CGFloat, maximum: CGFloat) -> CGFloat {
    guard minimum <= maximum else { return (minimum + maximum) / 2 }
    return min(max(value, minimum), maximum)
  }
}

enum RingMotion {
  // A short, well-damped spring gives the opening a little weight without repeated bouncing.
  static let appear = Animation.spring(response: 0.38, dampingFraction: 0.82)
  // Hover settles faster than presentation so the highlighted action follows the pointer.
  static let hover = Animation.spring(response: 0.24, dampingFraction: 0.86)
  static let change = Animation.spring(response: 0.32, dampingFraction: 0.9)
  static let dismiss = Animation.easeOut(duration: 0.16)
  static let reduced = Animation.easeOut(duration: 0.12)
  // Repositioning near a display edge should settle alongside the control's hover motion.
  static let windowAdjustmentDuration: TimeInterval = 0.2
}

/// White edges and desaturated blue accents sit over live macOS glass.
enum RingColor {
  static let ink = Color.white.opacity(0.94)
  static let secondary = Color.white.opacity(0.60)
  static let faint = Color.white.opacity(0.32)
  static let edge = Color.white.opacity(0.18)
  static let seal = Color(red: 0.57, green: 0.77, blue: 1)
  static let open = Color(red: 0.77, green: 0.72, blue: 1)
  static let success = Color(red: 0.64, green: 0.91, blue: 0.80)
  static let warning = Color(red: 1, green: 0.79, blue: 0.55)
  static let glassTop = Color.white.opacity(0.08)
  static let glassBottom = Color.black.opacity(0.22)
  static let smoke = Color.black.opacity(0.24)
  static let hubTint = Color.black.opacity(0.32)
}
