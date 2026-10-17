// @vitest-environment jsdom

import { describe, expect, it } from "vitest";

import { isAppFloatingSurfaceTarget } from "./utils";

describe("isAppFloatingSurfaceTarget", () => {
	it("matches targets inside app floating surfaces", () => {
		const surface = document.createElement("div");
		const button = document.createElement("button");
		surface.dataset.appFloatingSurface = "";
		surface.append(button);
		document.body.append(surface);

		expect(isAppFloatingSurfaceTarget(button)).toBe(true);

		surface.remove();
	});

	it("ignores regular document targets", () => {
		const button = document.createElement("button");
		document.body.append(button);

		expect(isAppFloatingSurfaceTarget(button)).toBe(false);

		button.remove();
	});
});
