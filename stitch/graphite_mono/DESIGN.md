# Design System Specification: The Intelligent Workspace

## 1. Overview & Creative North Star
**Creative North Star: "The Architectural Void"**

This design system is not merely a collection of components; it is a high-performance environment designed to disappear. By leveraging the "Architectural Void" philosophy, we prioritize the developer’s code and the AI’s insights over the UI itself. We move beyond the "template" look by utilizing intentional asymmetry, varying tonal depths, and a rejection of traditional structural lines. 

The experience must feel like a custom-machined piece of hardware: weighted, precise, and seamless. We achieve this through "vibrancy" (macOS-style transparency), high-contrast typography scales that command attention, and a "No-Line" layout strategy that relies on light and shadow rather than strokes.

## 2. Colors & Surface Philosophy
The palette is rooted in 'Graphite' and 'Space Gray' tones, utilizing the Material 3 tonal container logic to create a sophisticated, layered environment.

### Core Palette (Dark Mode Optimized)
*   **Background:** `#131313` (The base void)
*   **Primary (Accent):** `#adc6ff` (A soft, electric Indigo-Blue)
*   **Surface Container (Low):** `#1b1b1c`
*   **Surface Container (High):** `#2a2a2a`
*   **Tertiary (Syntax/AI):** `#ffb595` (Warmth to balance the cool grays)

### The "No-Line" Rule
Prohibit the use of 1px solid borders for sectioning. Structural boundaries must be defined solely through:
1.  **Background Shifts:** A `surface-container-low` sidebar sitting against a `surface` editor.
2.  **Tonal Transitions:** Using depth to imply a break in context.
3.  **Negative Space:** Using the spacing scale to create invisible gutters.

### The "Glass & Gradient" Rule
To capture the premium macOS "Vibrancy," the sidebar and floating panels must use Glassmorphism.
*   **Formula:** `surface-container-low` at 70% opacity + 30px Backdrop Blur.
*   **Signature Textures:** Main Action Buttons (CTAs) should utilize a subtle linear gradient from `primary` to `primary-container` (top-to-bottom) to provide a "machined" metallic sheen.

## 3. Typography: Editorial Precision
We utilize **Inter** (as the web-safe equivalent to SF Pro) to create an authoritative, editorial feel. 

| Role | Token | Weight | Size | Letter Spacing |
| :--- | :--- | :--- | :--- | :--- |
| **Display** | `display-lg` | 700 (Bold) | 3.5rem | -0.04em |
| **Headline** | `headline-sm` | 600 (Semi) | 1.5rem | -0.02em |
| **Title** | `title-md` | 500 (Medium) | 1.125rem | 0 |
| **Body** | `body-md` | 400 (Regular) | 0.875rem | +0.01em |
| **Label** | `label-sm` | 600 (Semi) | 0.6875rem | +0.05em (Caps) |

**Hierarchy Note:** Use `display-lg` sparingly for empty states or AI "thinking" modes to create a sense of scale. Body text should maintain a generous line-height (1.6) for maximum readability during long coding sessions.

## 4. Elevation & Depth
Depth in this system is achieved through **Tonal Layering**, not structural scaffolding.

*   **The Layering Principle:** Treat the UI as stacked sheets of frosted glass.
    *   *Level 0:* `surface-container-lowest` (The desk/base).
    *   *Level 1:* `surface` (The main application window).
    *   *Level 2:* `surface-container-high` (Floating modals or popovers).
*   **Ambient Shadows:** For floating elements, use "The Breath Shadow": `0px 20px 40px rgba(0, 0, 0, 0.4)`. Never use pure black shadows; tint them with the `on-surface` color at 4% opacity to mimic natural light.
*   **The Ghost Border:** If a separator is required for accessibility, use the `outline-variant` token at **15% opacity**. It should be felt, not seen.

## 5. Components

### Sidebar & Navigation
*   **Visuals:** Use the vibrancy effect (70% opacity). Icons must be thin-stroke (SF Symbols style).
*   **Interaction:** Active states use a "Pill" background (`secondary-container`) with a `xl` (0.75rem) border radius.

### The AI Chat Input
*   **Styling:** A floating `surface-container-highest` pill. No border. 
*   **Focus State:** Instead of a glow, use a 1px `primary` ghost-border at 40% opacity and increase the backdrop blur intensity.

### Buttons
*   **Primary:** Gradient-filled (`primary` to `primary-container`), `md` (0.375rem) radius. White text (`on-primary`).
*   **Secondary:** Ghost style. No background, `primary` text. On hover, a subtle `surface-variant` background appears.

### Code Editor (Syntax Highlighting)
*   **Background:** `surface-container-lowest` (#0e0e0e).
*   **Keywords:** `primary` (#adc6ff).
*   **Functions/Methods:** `tertiary` (#ffb595).
*   **Comments:** `outline` (#8b90a0).

### Segmented Controls (macOS Style)
*   **Container:** `surface-container-high`, `lg` radius.
*   **Active Tab:** A physically raised `surface-bright` chip with a soft ambient shadow.

## 6. Do’s and Don’ts

### Do
*   **Do** use asymmetrical layouts for AI responses (e.g., AI text aligned left, user code snippets slightly offset) to create an editorial feel.
*   **Do** use `9999px` (full) radius for toggle switches and chips to contrast against the `xl` (12px) radius of main windows.
*   **Do** prioritize vertical white space over dividers. If you think you need a line, add 16px of space instead.

### Don't
*   **Don't** use pure `#000000` or high-contrast white borders. It breaks the "vibrancy" illusion.
*   **Don't** use standard "Drop Shadows" with small blurs. If an object is "up," it must be soft and atmospheric.
*   **Don't** use "Information" blue for links. Use the `primary` indigo-blue to maintain the signature color profile.