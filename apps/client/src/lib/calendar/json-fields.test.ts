import { describe, expect, it } from "vitest";
import {
  parseJsonEventOrganizer,
  parseJsonGeoCoordinates,
  parseJsonNotificationMinutes,
  parseJsonStringArray,
  parseJsonStringRecord,
  safeJsonParse,
} from "./json-fields";

describe("calendar JSON validation", () => {
  it("parses unknown preservation payloads without assigning a domain type", () => {
    expect(safeJsonParse('{"custom":true}')).toEqual({ custom: true });
    expect(safeJsonParse("{{")).toBeUndefined();
  });

  it("rejects valid JSON with the wrong array element types", () => {
    expect(parseJsonStringArray('["work","team"]')).toEqual(["work", "team"]);
    expect(parseJsonStringArray('"work"')).toBeUndefined();
    expect(parseJsonStringArray('["work",1]')).toBeUndefined();

    expect(parseJsonNotificationMinutes("[-1,0,15,60]")).toEqual([-1, 0, 15, 60]);
    expect(parseJsonNotificationMinutes('[15,"60"]')).toBeUndefined();
    expect(parseJsonNotificationMinutes("[1.5]")).toBeUndefined();
  });

  it("accepts only string-valued extended property records", () => {
    expect(parseJsonStringRecord('{"x-app-key":"value"}')).toEqual({
      "x-app-key": "value",
    });
    expect(parseJsonStringRecord("[]")).toBeUndefined();
    expect(parseJsonStringRecord('{"x-app-key":1}')).toBeUndefined();
  });

  it("bounds geographic coordinates", () => {
    expect(parseJsonGeoCoordinates('{"lat":25.6,"lng":-100.3}')).toEqual({
      lat: 25.6,
      lng: -100.3,
    });
    expect(parseJsonGeoCoordinates('{"lat":91,"lng":0}')).toBeUndefined();
    expect(parseJsonGeoCoordinates('{"lat":0,"lng":181}')).toBeUndefined();
    expect(parseJsonGeoCoordinates('{"lat":"25.6","lng":-100.3}')).toBeUndefined();
  });

  it("requires a non-empty organizer email and an optional string name", () => {
    expect(parseJsonEventOrganizer('{"name":"Owner","email":"owner@example.com"}')).toEqual({
      name: "Owner",
      email: "owner@example.com",
    });
    expect(parseJsonEventOrganizer('{"name":null,"email":"owner@example.com"}')).toEqual({
      email: "owner@example.com",
    });
    expect(parseJsonEventOrganizer('{"name":42,"email":"owner@example.com"}')).toBeUndefined();
    expect(parseJsonEventOrganizer('{"email":""}')).toBeUndefined();
    expect(parseJsonEventOrganizer('{"email":"   "}')).toBeUndefined();
  });
});
