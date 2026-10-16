const USER_DB_URL = "sqlite:ganbaru.sqlite";

let initialized = false;

/** Return the mobile user database URL after startup has initialized it. */
export function dbUrl(): string {
  if (!initialized) {
    throw new Error("dbUrl() called before ensureDbUrl()");
  }
  return USER_DB_URL;
}

/** Initialize the fixed app-private mobile database URL. */
export async function ensureDbUrl(): Promise<string> {
  initialized = true;
  return USER_DB_URL;
}
