---
name: food-delivery
description: Food delivery / takeout app anatomy - home feed, store page, dish rows, cart bar
phase: [generation]
trigger:
  keywords: [外卖, 点餐, 点单, 餐厅, 饭店, 菜品, 菜单, 奶茶, 美食, 商家, takeout, take-out, food delivery, delivery app, food ordering, restaurant, dishes]
priority: 26
budget: 1500
category: domain
---

FOOD DELIVERY / TAKEOUT — what the leading apps converge on. Sources: Baymard Food Delivery & Takeout research (8 leading apps, 390+ UX parameters), DoorDash Merchant menu data, 美团 / 饿了么 product teardowns, 优设 金刚区 analysis. Where these rules differ from the generic mobile rules (icon tiles, list-row alignment), these win. A restaurant web page uses only the photo, dish-row and store-header rules.

SCOPE FIRST: a single cafe/brand ordering home is not a multi-store marketplace. Use the supplied store/pickup details, one hero drink, the supplied menu and compact categories. Skip reorder history, flash deals, multi-store lists and delivery metadata unless requested. Never treat the numeric examples below as facts; unknown ratings, distance, sales and discounts must be omitted.

Food photography carries the appetite: give stores and dishes specific `imagePrompt` + `imageSearchQuery` values ("mapo tofu in clay bowl, overhead"). Dish shots share one angle, crop and lighting. Category navigation uses ONE coherent icon/illustration family; photographic categories are optional only when a matching set is available, not unrelated stock thumbnails in circles. Keep store-list labels on the page surface; reserve text-over-photo for one deliberate hero with a subtle scrim. Accent colour is for prices, add buttons, checkout, active tabs and promo text; at most ONE large accent-filled block per screen.

Multi-screen continuity: reuse the selected store identity, dish names and unit prices from home through menu and checkout. Derive each line subtotal and the final total from one basket; the fee-card total and submit-bar total must match. Do not invent different prices or merchants on the next screen.

HOME, top to bottom:
1. Address + search as one block. Address row: pin icon, 12px "送至", 15-16px semibold address, chevron. Then a full-width 40-44px search field whose placeholder carries a benefit ("满30减12 · 搜索美食"; 美团 puts the discount in the search box).
2. Category navigation: honor exact counts first — 九宫格 is nine entries in 3×3. Otherwise choose 4-5 columns for the given labels. Use one consistent 40-48px icon or illustration family with 12px labels; keep colors restrained and strokes/scale consistent. No independent stock-photo collage. Preserve the grid and scroll lower content instead of changing the requested inventory.
3. Order again: returning users look for reorder on the home page (Baymard; DoorDash "Order Again"). Use a horizontal row of 2-3 recent stores, each with photo, name and a small "再来一单" button.
4. One promo moment: a flash deal with REAL countdown digits as plain text ("02:15:38", tabular cells), a deal price beside a struck-through price, and one dish photo. Not an empty orange slab.
5. Sort/filter row: 综合排序 · 距离 · 销量 · 筛选, chips 32-36px. About half of users filter sometimes (Baymard).
6. Store list rows (no shadow box per row; separate rows with 16-20px spacing or a hairline):
   - 80-88px photo (storefront or signature dish), radius 10-12.
   - Name 16-17px semibold.
   - Meta line 12-13px: ★4.8 · 月售3200+ · 30分钟 · 1.2km. Delivery TIME is the deciding figure: give it weight or the accent.
   - Cost line 12px muted: 起送¥20 · 配送¥3.
   - Up to 3 promo tags: 满30减12, 新客立减5. Red/orange text on a light tint, 18-20px tall, radius 4.
   - On 1-2 featured stores, add a strip of 3 signature-dish thumbnails (56-64px, price under each; 美团 "发现好菜"). It breaks the identical-row rhythm.

STORE PAGE:
- Full-bleed store photo 180-220px under the status bar, with back/favorite/share as 36px translucent circles on it. A 56-64px store logo (radius 12, white 2px border) overlaps the photo's bottom edge.
- Info block: name 20-22px bold; ★ · 月售 · 时间 · 距离; promo tags; one announcement line.
- 商家推荐 / "Most ordered": a horizontal carousel of 3-4 signature dishes, each card 132-140px wide with a 1:1 photo, name and price (饿了么 recommended-dish module).
- Menu layout:
  - Chinese apps use a LEFT CATEGORY RAIL (80-92px wide, tinted background; the selected item is white with a 3px accent bar; labels 13-14px) beside the dish list. Use it for Chinese briefs.
  - Western apps use sticky horizontal category tabs instead.
- Dish row, `alignItems: start`:
  - 88-96px square photo, radius 10.
  - Name 15-16px semibold.
  - One-line description 12-13px muted.
  - "月售 1260" 12px muted.
  - Price 17-18px bold, with an optional struck-through original.
  - 28-32px accent circle "+" at the bottom-right; it becomes a − 1 + stepper once the dish is in the cart. Padding keeps the touch target at least 44px (HIG).
- Cart bar: `role="cart-bar"`, outside the scroll body as the final root sibling. A floating dark pill, 56-60px tall, radius full, inset 12-16px from the sides and bottom. Checkout's persistent submit area uses `role="bottom-action-bar"` with the same viewport ownership.
  - Cart disc 44-48px overlapping the pill's left edge, with an 18px count badge.
  - Total 18-20px bold white, then "另需配送费¥3" 11-12px.
  - Checkout CTA on the right. Below the minimum order, show "还差¥8起送" in place of the CTA (饿了么 shortfall prompt).

DO NOT: mix photo, emoji and unrelated icon styles within one category grid; give every store row an identical white shadow card; paint the whole header orange; leave dish or store photos out; change merchant or basket facts between screens; use more than one large accent-filled block per screen.
