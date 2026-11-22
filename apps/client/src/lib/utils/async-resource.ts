/**
 * The lifecycle of data that one request at a time loads, with the request in flight named by a token inside the
 * loading variant. A newer request, or a reset, replaces the token, so a stale response can never settle the
 * resource. Data stays in every variant, so a reload keeps the loaded data visible until a response replaces it.
 */
export type AsyncResource<T> =
  | { readonly status: "idle"; readonly data: T }
  | { readonly status: "loading"; readonly data: T; readonly token: AsyncRequestToken }
  | { readonly status: "failed"; readonly data: T; readonly error: string };

/** Names one request; every request gets a token no other request shares. */
export type AsyncRequestToken = symbol;

/** A resource paired with the token of the request that just started. */
export interface StartedAsyncRequest<T> {
  readonly resource: AsyncResource<T>;
  readonly token: AsyncRequestToken;
}

/**
 * An idle resource holding `data`. Replacing a resource with it also supersedes any request in flight.
 *
 * @param data - The data to show.
 */
export function idleAsyncResource<T>(data: T): AsyncResource<T> {
  return { status: "idle", data };
}

/**
 * Starts a request that supersedes any request in flight, keeping the current data and clearing any error.
 *
 * @param resource - The resource to load.
 * @returns The loading resource and the token its response must present.
 */
export function startAsyncRequest<T>(resource: AsyncResource<T>): StartedAsyncRequest<T> {
  const token: AsyncRequestToken = Symbol("async request");
  return { resource: { status: "loading", data: resource.data, token }, token };
}

/**
 * Whether `token` names the request `resource` is waiting for.
 *
 * @param resource - The resource.
 * @param token - The token a response carries.
 */
export function isCurrentAsyncRequest<T>(resource: AsyncResource<T>, token: AsyncRequestToken): boolean {
  return resource.status === "loading" && resource.token === token;
}

/**
 * Settles the request `token` with `data`. A stale token leaves the resource unchanged.
 *
 * @param resource - The resource.
 * @param token - The token of the response.
 * @param data - The loaded data.
 */
export function resolveAsyncRequest<T>(
  resource: AsyncResource<T>,
  token: AsyncRequestToken,
  data: T,
): AsyncResource<T> {
  return isCurrentAsyncRequest(resource, token) ? idleAsyncResource(data) : resource;
}

/**
 * Fails the request `token` with `error`, keeping the current data unless `data` replaces it. A stale token leaves
 * the resource unchanged.
 *
 * @param resource - The resource.
 * @param token - The token of the response.
 * @param error - The error to show.
 * @param data - The data to show with the error, the current data when omitted.
 */
export function failAsyncRequest<T>(
  resource: AsyncResource<T>,
  token: AsyncRequestToken,
  error: string,
  data: T = resource.data,
): AsyncResource<T> {
  return isCurrentAsyncRequest(resource, token) ? { status: "failed", data, error } : resource;
}

/**
 * Ends the request `token` without applying its response, for a response that no longer belongs to what is shown.
 * A stale token leaves the resource unchanged.
 *
 * @param resource - The resource.
 * @param token - The token of the response.
 */
export function abandonAsyncRequest<T>(resource: AsyncResource<T>, token: AsyncRequestToken): AsyncResource<T> {
  return isCurrentAsyncRequest(resource, token) ? idleAsyncResource(resource.data) : resource;
}

/**
 * Replaces the data in place, keeping the status and any request in flight, for a local write to loaded data.
 *
 * @param resource - The resource.
 * @param update - Derives the new data from the current data.
 */
export function updateAsyncResourceData<T>(resource: AsyncResource<T>, update: (data: T) => T): AsyncResource<T> {
  return { ...resource, data: update(resource.data) };
}

/**
 * Whether a request is in flight.
 *
 * @param resource - The resource.
 */
export function isAsyncResourceLoading<T>(resource: AsyncResource<T>): boolean {
  return resource.status === "loading";
}

/**
 * The error of the last request, or `null` unless it failed.
 *
 * @param resource - The resource.
 */
export function asyncResourceError<T>(resource: AsyncResource<T>): string | null {
  return resource.status === "failed" ? resource.error : null;
}
