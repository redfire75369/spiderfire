declare interface QueuingStrategyInit {
	highWaterMark: number;
}

declare class ByteLengthQueuingStrategy {
	constructor(init: QueuingStrategyInit);

	get highWaterMark(): number;

	get size(): (chunk: any) => number;
}

declare class CountQueuingStrategy {
	constructor(init: QueuingStrategyInit);

	get highWaterMark(): number;

	get size(): () => number;
}
