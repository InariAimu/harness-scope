/** Keep edits through snapshot refreshes, and append newly discovered accounts. */
export function reconcileProviderSidebarOrder(order: readonly string[], accountIds: readonly string[]) {
  const known = new Set(accountIds);
  const seen = new Set<string>();
  return [...order, ...accountIds].filter(id => {
    if (!known.has(id) || seen.has(id)) return false;
    seen.add(id);
    return true;
  });
}

/** Swap visible neighbors without changing hidden accounts' slots. */
export function moveProviderSidebarAccount(
  order: readonly string[], visibleIds: readonly string[], id: string, direction: -1 | 1,
) {
  const position = visibleIds.indexOf(id);
  const neighbor = visibleIds[position + direction];
  if (position < 0 || neighbor === undefined) return [...order];
  const from = order.indexOf(id);
  const to = order.indexOf(neighbor);
  if (from < 0 || to < 0) return [...order];
  const moved = [...order];
  [moved[from], moved[to]] = [moved[to], moved[from]];
  return moved;
}
