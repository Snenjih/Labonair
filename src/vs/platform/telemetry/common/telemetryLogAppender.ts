/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../base/common/lifecycle.js';
import { IEnvironmentService } from '../../environment/common/environment.js';
import { ILoggerService } from '../../log/common/log.js';
import { IProductService } from '../../product/common/productService.js';
import { ITelemetryAppender } from './telemetryUtils.js';

export class TelemetryLogAppender extends Disposable implements ITelemetryAppender {

	constructor(
		_prefix: string,
		_remote: boolean,
		@ILoggerService _loggerService: ILoggerService,
		@IEnvironmentService _environmentService: IEnvironmentService,
		@IProductService _productService: IProductService,
	) {
		super();
		// Labonair: Telemetry is completely disabled - minimal initialization
	}

	flush(): Promise<void> {
		return Promise.resolve();
	}

	log(eventName: string, data: unknown): void {
		// Labonair: Telemetry is completely disabled - No-Op implementation
		return;
	}
}

