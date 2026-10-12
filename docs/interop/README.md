# Interoperability

This section documents file formats and external-client behavior. Standards define correctness. Client-specific observations explain practical compatibility, but never override a standard.

## Supported domains

- [iCalendar](./icalendar/README.md): offline `.ics` import, structured preservation, app projection, export, recurrence, timezones, conformance, and client observations.
- [Notes clipboard](./notes-clipboard.md): Markdown and semantic HTML exchange, supported content, limitations, and verification evidence.

Additional formats should receive their own folder only when they have a durable standards scope, implementation boundary, and conformance strategy.

## Document roles

- Standards and design documents define the intended interoperability contract.
- Conformance documents record implementation status and known gaps against current source and tests.
- Client documents record dated observations from real applications and services.

Compatibility claims should be semantic and evidence-based. Ganbaru AI does not promise byte-for-byte reproduction of imported files, because legal serializers may normalize case, escaping, ordering, and line folding without changing meaning.
