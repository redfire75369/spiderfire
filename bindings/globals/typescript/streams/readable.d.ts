declare type UnderlyingSourceStartCallback = (controller: ReadableStreamController) => any;
declare type UnderlyingSourcePullCallback = (controller: ReadableStreamController) => Promise<void>;
declare type UnderlyingSourceCancelCallback = (reason?: any) => Promise<void>;

declare type ReadableStreamType = "bytes";

declare interface UnderlyingSource {
	start?: UnderlyingSourceStartCallback,
	pull?: UnderlyingSourcePullCallback,
	cancel?: UnderlyingSourceCancelCallback,
	type?: ReadableStreamType,
	autoAllocateChunkSize?: number,
}

declare type QueuingStrategySize = (chunk: any) => number;

declare interface QueuingStrategy {
	highWaterMark?: number,
	size: QueuingStrategySize,
}

declare type ReadableStreamReaderMode = "byob";

declare interface ReadableStreamGetReaderOptions {
	mode?: ReadableStreamReaderMode
}

declare class ReadableStream {
	constructor(underlyingSource?: UnderlyingSource, strategy?: QueuingStrategy);

	get locked(): boolean;

	cancel(reason?: any): Promise<void>;

	getReader(options?: ReadableStreamGetReaderOptions): ReadableStreamReader;

	tee(): [ReadableStream, ReadableStream];
}

declare interface ReadableStreamReadResult {
	value: any,
	done: boolean,
}

declare abstract class ReadableStreamGenericReader {
	get closed(): Promise<void>;

	cancel(reason?: any): Promise<void>;
}

declare class ReadableStreamDefaultReader extends ReadableStreamGenericReader {
	constructor(stream: ReadableStream);

	read(): Promise<ReadableStreamReadResult>;

	releaseLock(): void;
}

declare interface ReadableStreamBYOBReaderReadOptions {
	min?: number,
}

declare class ReadableStreamBYOBReader extends ReadableStreamGenericReader {
	constructor(stream: ReadableStream);

	read(view: ArrayBufferView, options?: ReadableStreamBYOBReaderReadOptions): Promise<ReadableStreamReadResult>;

	releaseLock(): void;
}

declare type ReadableStreamReader = ReadableStreamDefaultReader | ReadableStreamBYOBReader;

declare class ReadableStreamDefaultController {
	get desiredSize(): number | null;

	close(): void;

	enqueue(chunk?: any): void;

	error(e?: any): void;
}


declare class ReadableByteStreamController {
	get byobRequest(): ReadableStreamBYOBRequest | null;

	get desiredSize(): number | null;

	close(): void;

	enqueue(chunk: ArrayBufferView): void;

	error(e?: any): void;
}

declare type ReadableStreamController = ReadableStreamDefaultController | ReadableByteStreamController;

declare class ReadableStreamBYOBRequest {
	get view(): ArrayBufferView | null;

	respond(bytesWritten: number): void;

	respondInto(view: ArrayBufferView): void;
}
