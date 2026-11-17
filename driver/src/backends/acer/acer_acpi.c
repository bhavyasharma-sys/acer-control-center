/*!
 * Copyright (c) 2026 Stanly Silas <vstanlysilas@gmail.com>
 *
 * This file is part of acer-keyboard.
 *
 * acer-keyboard is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This software is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this software.  If not, see <https://www.gnu.org/licenses/>.
 */
#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt
#include <linux/kernel.h>
#include <linux/module.h>
#include <linux/acpi.h>
#include <linux/platform_device.h>
#include <linux/pm.h>
#include <linux/version.h>
#include <linux/mutex.h>
#include <linux/string.h>
#include "acer_interfaces.h"

#define DRIVER_NAME			"acer_acpi"

#define ACER_DCHU_PATH "\\_SB.DCHU"
#define ACER_DCHU_FAN_MODE_FUNCTION 0x79
#define ACER_DCHU_BUFFER_SIZE 0x100

static DEFINE_MUTEX(acer_dchu_eval_lock);

static const guid_t acer_dchu_guid =
	GUID_INIT(0x93f224e4, 0xfbdc, 0x4bbf,
		  0xad, 0xd6, 0xdb, 0x71, 0xbd, 0xc0, 0xaf, 0xad);

struct acer_acpi_driver_data_t {
	acpi_handle handle;
	struct acer_interface_t *acer_interface;
};

static struct acer_acpi_driver_data_t *active_driver_data = NULL;

static void acer_acpi_notify(acpi_handle handle, u32 event, void *context);

static int acer_acpi_evaluate(acpi_handle handle, u8 cmd, u32 arg, union acpi_object **result)
{
	int status;
	u64 dsm_rev_dummy = 0x00; // Dummy 0 value since not used
	u64 dsm_func = cmd;
	// Integer package data for argument
	union acpi_object dsm_argv4_package_data[] = {
		{
			.integer.type = ACPI_TYPE_INTEGER,
			.integer.value = arg
		}
	};

	// Package argument
	union acpi_object dsm_argv4 = {
		.package.type = ACPI_TYPE_PACKAGE,
		.package.count = 1,
		.package.elements = dsm_argv4_package_data
	};

	union acpi_object *out_obj;

	guid_t acer_acpi_dsm_uuid;

	status = guid_parse(ACER_ACPI_DSM_UUID, &acer_acpi_dsm_uuid);
	if (status < 0)
		return -ENOENT;

	if (handle == NULL)
		return -ENODEV;

	out_obj = acpi_evaluate_dsm(handle, &acer_acpi_dsm_uuid, dsm_rev_dummy, dsm_func, &dsm_argv4);
	if (!out_obj) {
		pr_err("failed to evaluate _DSM\n");
		status = -1;
	}
	else {
		if (!IS_ERR_OR_NULL(result)) {
			*result = out_obj;
		}
	}

	return status;
}

static int acer_acpi_interface_method_call(u8 cmd, u32 arg, union acpi_object **result_value)
{
	int status = 0;

	if (!IS_ERR_OR_NULL(active_driver_data)) {
		status = acer_acpi_evaluate(active_driver_data->handle, cmd, arg, result_value);
	} else {
		pr_err("acpi method call exec, no driver data found\n");
		pr_err("..for method_call: %0#4x arg: %0#10x\n", cmd, arg);
		status = -ENODATA;
	}
	// pr_debug("acer_acpi method_call: %0#4x arg: %0#10x result: %0#10x\n", cmd, arg, !IS_ERR_OR_NULL(result_value) ? *result_value : 0);

	return status;
}

struct acer_interface_t acer_acpi_interface = {
	.string_id = ACER_INTERFACE_ACPI_STRID,
	.method_call = acer_acpi_interface_method_call,
};

static int acer_acpi_probe(struct platform_device *pdev)
{
	struct acer_acpi_driver_data_t *driver_data;
	struct acpi_device *device = ACPI_COMPANION(&pdev->dev);
	int ret;

	if (!device) {
		pr_err("no ACPI companion device\n");
		return -ENODEV;
	}

	driver_data = devm_kzalloc(&pdev->dev, sizeof(*driver_data), GFP_KERNEL);
	if (!driver_data)
		return -ENOMEM;

	driver_data->handle = device->handle;
	driver_data->acer_interface = &acer_acpi_interface;

	ret = devm_acpi_install_notify_handler(&pdev->dev, ACPI_ALL_NOTIFY,
					      acer_acpi_notify, NULL);
	if (ret) {
		pr_err("failed to install notify handler: %d\n", ret);
		return ret;
	}

	active_driver_data = driver_data;

	pr_debug("acer_acpi driver probe\n");

	// Add this interface
	acer_keyboard_add_interface(&acer_acpi_interface);

	pr_info("interface initialized\n");

	return 0;
}

static void acer_acpi_remove(struct platform_device *pdev)
{
	pr_debug("acer_acpi driver remove\n");
	acer_keyboard_remove_interface(&acer_acpi_interface);
	active_driver_data = NULL;
}

static void acer_acpi_notify(acpi_handle handle, u32 event, void *context)
{
	u32 event_value = 0;
	union acpi_object *out_obj;
	bool event_valid = false;
	int status;

	status = acer_acpi_evaluate(handle, 0x01, 0, &out_obj);
	if (!status) {
		if (out_obj->type == ACPI_TYPE_INTEGER) {
			event_value = (u32)out_obj->integer.value;
			event_valid = true;
			pr_info("firmware hotkey event: source=ACPI notification=0x%04x code=0x%04x\n",
				event, event_value);
		} else {
			pr_err("return type not integer, use acer_evaluate_method2\n");
		}
		ACPI_FREE(out_obj);
		pr_debug("acer_acpi event: %0#6x, acer event value: %0#6x\n", event, event_value);

		if (event_valid && !IS_ERR_OR_NULL(acer_acpi_interface.event_callb)) {
			/* `event` is the ACPI notification number; the DSM result is
			 * the Acer hotkey code consumed by the shared keymap. */
			acer_acpi_interface.event_callb(event_value);
		}
	}
}

#ifdef CONFIG_PM
static int driver_suspend_callb(struct device *dev)
{
	pr_debug("driver suspend\n");
	return 0;
}

static int driver_resume_callb(struct device *dev)
{
	pr_debug("driver resume\n");
	return 0;
}

static DEFINE_SIMPLE_DEV_PM_OPS(acer_driver_pm_ops, driver_suspend_callb, driver_resume_callb);
#endif

static const struct acpi_device_id acer_acpi_device_ids[] = {
	{ACER_ACPI_RESOURCE_HID, 0},
	{"", 0}
};

MODULE_DEVICE_TABLE(acpi, acer_acpi_device_ids);

static struct platform_driver acer_acpi_driver = {
	.probe = acer_acpi_probe,
	.remove = acer_acpi_remove,
	.driver = {
		.name = DRIVER_NAME,
		.acpi_match_table = acer_acpi_device_ids,
#ifdef CONFIG_PM
		.pm = &acer_driver_pm_ops,
#endif
	},
};

int acer_dchu_set_fan_mode(u32 mode)
{
	union acpi_object args[4] = {};
	union acpi_object package_element = {};
	struct acpi_object_list input;
	struct acpi_buffer output = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *obj;
	acpi_handle dchu_handle;
	acpi_status status;
	u8 buffer[ACER_DCHU_BUFFER_SIZE] = { 0 };
	u32 payload;

	if (mode != 0 && mode != 1)
		return -EINVAL;

	/* DCHU fan mode: 0 = Auto, 1 = Max. */
	payload = (0x01u << 24) | mode;
	memcpy(buffer, &payload, sizeof(payload));

	args[0].type = ACPI_TYPE_BUFFER;
	args[0].buffer.pointer = (u8 *)&acer_dchu_guid;
	args[0].buffer.length = sizeof(acer_dchu_guid);

	args[1].type = ACPI_TYPE_INTEGER;
	args[1].integer.value = 1;

	args[2].type = ACPI_TYPE_INTEGER;
	args[2].integer.value = ACER_DCHU_FAN_MODE_FUNCTION;

	package_element.type = ACPI_TYPE_BUFFER;
	package_element.buffer.pointer = buffer;
	package_element.buffer.length = sizeof(buffer);

	args[3].type = ACPI_TYPE_PACKAGE;
	args[3].package.count = 1;
	args[3].package.elements = &package_element;

	input.count = ARRAY_SIZE(args);
	input.pointer = args;

	mutex_lock(&acer_dchu_eval_lock);

	status = acpi_get_handle(NULL, ACER_DCHU_PATH, &dchu_handle);
	if (ACPI_FAILURE(status)) {
		mutex_unlock(&acer_dchu_eval_lock);
		pr_err("DCHU handle unavailable: %s\n",
		       acpi_format_exception(status));
		return -ENODEV;
	}

	status = acpi_evaluate_object(
		dchu_handle, "_DSM", &input, &output);

	mutex_unlock(&acer_dchu_eval_lock);

	if (ACPI_FAILURE(status)) {
		pr_err("DCHU fan mode %u failed: %s\n",
		       mode, acpi_format_exception(status));
		if (output.pointer)
			ACPI_FREE(output.pointer);
		return -EIO;
	}

	if (output.pointer) {
		obj = output.pointer;

		if (obj->type == ACPI_TYPE_INTEGER &&
		    obj->integer.value != ACER_DCHU_FAN_MODE_FUNCTION) {
			pr_err("Unexpected DCHU fan-mode response: 0x%llx\n",
			       obj->integer.value);
			ACPI_FREE(output.pointer);
			return -EIO;
		}

		ACPI_FREE(output.pointer);
	}

	return 0;
}
EXPORT_SYMBOL_GPL(acer_dchu_set_fan_mode);

int acer_dchu_read_status(u8 *buffer, size_t buffer_size)
{
	union acpi_object args[4] = {};
	union acpi_object package_element = {};
	struct acpi_object_list input;
	struct acpi_buffer output = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *obj;
	acpi_handle dchu_handle;
	acpi_status status;
	u8 payload[ACER_DCHU_BUFFER_SIZE] = { 0 };
	size_t copy_size;

	if (!buffer || buffer_size < 8)
		return -EINVAL;

	args[0].type = ACPI_TYPE_BUFFER;
	args[0].buffer.pointer = (u8 *)&acer_dchu_guid;
	args[0].buffer.length = sizeof(acer_dchu_guid);

	args[1].type = ACPI_TYPE_INTEGER;
	args[1].integer.value = 1;

	args[2].type = ACPI_TYPE_INTEGER;
	args[2].integer.value = 0x0c;

	package_element.type = ACPI_TYPE_BUFFER;
	package_element.buffer.pointer = payload;
	package_element.buffer.length = sizeof(payload);

	args[3].type = ACPI_TYPE_PACKAGE;
	args[3].package.count = 1;
	args[3].package.elements = &package_element;

	input.count = ARRAY_SIZE(args);
	input.pointer = args;

	mutex_lock(&acer_dchu_eval_lock);
	status = acpi_get_handle(NULL, ACER_DCHU_PATH, &dchu_handle);
	if (ACPI_FAILURE(status)) {
		mutex_unlock(&acer_dchu_eval_lock);
		return -ENODEV;
	}

	status = acpi_evaluate_object(dchu_handle, "_DSM", &input, &output);
	mutex_unlock(&acer_dchu_eval_lock);
	if (ACPI_FAILURE(status)) {
		if (output.pointer)
			ACPI_FREE(output.pointer);
		return -EIO;
	}
	if (!output.pointer)
		return -EIO;

	obj = output.pointer;
	if (obj->type != ACPI_TYPE_BUFFER || obj->buffer.length < 8) {
		ACPI_FREE(output.pointer);
		return -EIO;
	}

	memset(buffer, 0, buffer_size);
	copy_size = min_t(size_t, obj->buffer.length, buffer_size);
	memcpy(buffer, obj->buffer.pointer, copy_size);
	ACPI_FREE(output.pointer);
	return 0;
}
EXPORT_SYMBOL_GPL(acer_dchu_read_status);

module_platform_driver(acer_acpi_driver);

MODULE_AUTHOR("Stanly Silas <vstanlysilas@gmail.com>");
MODULE_DESCRIPTION("Driver for Acer ACPI interface");
MODULE_VERSION("0.1.0");
MODULE_LICENSE("GPL");
