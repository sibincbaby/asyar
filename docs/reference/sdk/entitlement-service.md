### 8.11 `EntitlementService` — Entitlement checks (always "not entitled")

**Runs in:** view only.

**Permission required:** `entitlements:read`

> **This build has no accounts or subscriptions.** The service still exists so published extensions that declare `entitlements:read` keep working, but it always reports "not entitled": `check()` resolves to `false` for every string and `getAll()` resolves to `[]`. Do not gate core functionality on it, and treat `false` as the normal case.

```typescript
interface IEntitlementService {
  /** Always resolves to `false` in this build. */
  check(entitlement: string): Promise<boolean>;

  /** Always resolves to `[]` in this build. */
  getAll(): Promise<string[]>;
}
```

**Usage:**

```typescript
// manifest.json
{
  "permissions": ["entitlements:read"]
}

// main.ts
const entitlements = context.getService<IEntitlementService>('entitlements');

// Always false in this build — keep a working fallback path
const entitled = await entitlements.check('my-feature'); // false

// Always an empty array in this build
const all = await entitlements.getAll(); // []
```

---
