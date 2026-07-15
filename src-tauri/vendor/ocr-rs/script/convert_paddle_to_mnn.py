import os
import sys
import subprocess
import argparse
import shutil
import yaml
from pathlib import Path


def check_dependencies():
    """Check required dependencies"""
    # Check paddle2onnx
    try:
        subprocess.run(['paddle2onnx', '--version'], 
                      capture_output=True, text=True, check=True)
    except (FileNotFoundError, subprocess.CalledProcessError):
        print("Error: paddle2onnx not found. Install: pip install paddle2onnx")
        return False
    
    # Check mnnconvert
    try:
        subprocess.run(['mnnconvert', '--version'], 
                      capture_output=True, text=True, check=True)
    except (FileNotFoundError, subprocess.CalledProcessError):
        print("Error: mnnconvert not found. Install from: https://github.com/alibaba/MNN")
        return False
    
    return True


def convert_paddle_to_onnx(model_dir):
    """Convert Paddle model to ONNX format"""
    model_path = Path(model_dir)
    inference_json = model_path / "inference.json"
    inference_pdiparams = model_path / "inference.pdiparams"
    output_onnx = model_path / "model.onnx"
    
    if not inference_json.exists() or not inference_pdiparams.exists():
        return False
    
    if output_onnx.exists():
        return True
    
    cmd = [
        'paddle2onnx',
        '--model_dir', '.',
        '--model_filename', 'inference.json',
        '--params_filename', 'inference.pdiparams',
        '--save_file', 'model.onnx'
    ]
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, cwd=str(model_path))
        if result.returncode != 0:
            print(f"  [ONNX] Failed: {result.stderr.strip().split(chr(10))[-1]}")
            return False
        print(f"  [ONNX] ✓")
        return True
    except Exception as e:
        print(f"  [ONNX] Error: {e}")
        return False


def convert_onnx_to_mnn(model_dir, use_fp16=True):
    """Convert ONNX model to MNN format"""
    model_path = Path(model_dir)
    input_onnx = model_path / "model.onnx"
    output_mnn = model_path / "model.mnn"
    
    if not input_onnx.exists():
        return False
    
    if output_mnn.exists():
        return True
    
    cmd = [
        'mnnconvert',
        '-f', 'ONNX',
        '--modelFile', 'model.onnx',
        '--MNNModel', 'model.mnn',
        '--bizCode', 'mnn'
    ]
    
    if use_fp16:
        cmd.append('--fp16')
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, cwd=str(model_path))
        if result.returncode != 0:
            print(f"  [MNN] Failed: {result.stderr.strip().split(chr(10))[-1]}")
            return False
        print(f"  [MNN] ✓ (fp16={use_fp16})")
        return True
    except Exception as e:
        print(f"  [MNN] Error: {e}")
        return False


def extract_character_dict(model_dir):
    """Extract character dictionary from inference.yml"""
    model_path = Path(model_dir)
    inference_yml = model_path / "inference.yml"
    output_txt = model_path / "ppocr_keys.txt"
    
    if not inference_yml.exists():
        return False
    
    if output_txt.exists():
        return True
    
    try:
        with open(inference_yml, 'r', encoding='utf-8') as f:
            data = yaml.safe_load(f)
        
        character_dict = None
        if 'PostProcess' in data:
            if isinstance(data['PostProcess'], dict):
                character_dict = data['PostProcess'].get('character_dict')
            elif isinstance(data['PostProcess'], list):
                for item in data['PostProcess']:
                    if isinstance(item, dict) and 'character_dict' in item:
                        character_dict = item['character_dict']
                        break
        
        if not character_dict:
            return False
        
        with open(output_txt, 'w', encoding='utf-8') as f:
            for char in character_dict:
                char_str = str(char) if char is not None else ''
                f.write(char_str + '\n')
        
        print(f"  [Dict] ✓ ({len(character_dict)} chars)")
        return True
    
    except Exception as e:
        print(f"  [Dict] Error: {e}")
        return False


def charset_filename_for_model(model_name):
    """Return runtime charset filename for known recognition model directories"""
    ppocrv6_charsets = {
        "PP-OCRv6_tiny_rec": "ppocr_keys_v6_tiny.txt",
        "PP-OCRv6_small_rec": "ppocr_keys_v6_small.txt",
        "PP-OCRv6_medium_rec": "ppocr_keys_v6_medium.txt",
    }
    if model_name in ppocrv6_charsets:
        return ppocrv6_charsets[model_name]

    if model_name in ("ch_PP-OCRv4_rec_infer", "PP-OCRv4_rec"):
        return "ppocr_keys_v4.txt"

    if model_name in ("PP-OCRv5_mobile_rec", "PP-OCRv5_mobile_rec_infer"):
        return "ppocr_keys_v5.txt"

    ppocrv5_prefixes = {
        "korean": "ppocr_keys_korean.txt",
        "latin": "ppocr_keys_latin.txt",
        "eslav": "ppocr_keys_eslav.txt",
        "th": "ppocr_keys_th.txt",
        "el": "ppocr_keys_el.txt",
        "en": "ppocr_keys_en.txt",
        "cyrillic": "ppocr_keys_cyrillic.txt",
        "arabic": "ppocr_keys_arabic.txt",
        "devanagari": "ppocr_keys_devanagari.txt",
        "ta": "ppocr_keys_ta.txt",
        "te": "ppocr_keys_te.txt",
    }
    for prefix, charset_filename in ppocrv5_prefixes.items():
        if model_name.startswith(f"{prefix}_PP-OCRv5_mobile_rec"):
            return charset_filename

    return None


def install_converted_model(model_dir, install_dir):
    """Install converted MNN and charset files into the runtime models directory"""
    model_path = Path(model_dir)
    install_path = Path(install_dir)
    output_mnn = model_path / "model.mnn"

    if not output_mnn.exists():
        print("  [Install] skipped (model.mnn missing)")
        return False

    install_path.mkdir(parents=True, exist_ok=True)

    target_mnn = install_path / f"{model_path.name}.mnn"
    shutil.copy2(output_mnn, target_mnn)
    installed = [target_mnn.name]

    charset_source = model_path / "ppocr_keys.txt"
    charset_filename = charset_filename_for_model(model_path.name)
    if charset_source.exists() and charset_filename:
        target_charset = install_path / charset_filename
        shutil.copy2(charset_source, target_charset)
        installed.append(target_charset.name)

    print(f"  [Install] ✓ {', '.join(installed)}")
    return True


def convert_model(model_dir, use_fp16=True):
    """Convert single model directory"""
    model_path = Path(model_dir)
    model_name = model_path.name
    
    print(f"\n{model_name}:")
    
    results = {
        'paddle_to_onnx': False,
        'onnx_to_mnn': False,
        'extract_dict': False
    }
    
    results['paddle_to_onnx'] = convert_paddle_to_onnx(model_path)
    
    if results['paddle_to_onnx']:
        results['onnx_to_mnn'] = convert_onnx_to_mnn(model_path, use_fp16)
    
    results['extract_dict'] = extract_character_dict(model_path)
    
    return results


def main():
    parser = argparse.ArgumentParser(
        description='Convert Paddle OCR models to MNN format',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Convert all models in ocr directory (with FP16)
  python convert_paddle_to_mnn.py
  
  # Specify OCR directory
  python convert_paddle_to_mnn.py --ocr-dir ./my_ocr_models

  # Convert and install runtime files into models/
  python convert_paddle_to_mnn.py --ocr-dir ./my_ocr_models --install-dir ./models
  
  # Disable FP16
  python convert_paddle_to_mnn.py --no-fp16
        """
    )
    
    parser.add_argument(
        '--ocr-dir',
        type=str,
        default='./ocr',
        help='OCR models root directory (default: ./ocr)'
    )
    
    parser.add_argument(
        '--no-fp16',
        action='store_true',
        help='Disable FP16 precision (default: enabled)'
    )

    parser.add_argument(
        '--install-dir',
        type=str,
        help='Copy converted runtime files into this models directory'
    )
    
    args = parser.parse_args()
    
    ocr_dir = Path(args.ocr_dir)
    use_fp16 = not args.no_fp16
    
    print(f"Paddle to MNN Converter")
    print(f"OCR dir: {ocr_dir.absolute()}")
    print(f"FP16: {use_fp16}")
    if args.install_dir:
        print(f"Install dir: {Path(args.install_dir).absolute()}")
    print()
    
    if not check_dependencies():
        sys.exit(1)
    
    if not ocr_dir.exists():
        print(f"Error: OCR directory not found: {ocr_dir}")
        sys.exit(1)
    
    model_dirs = [d for d in ocr_dir.iterdir() if d.is_dir()]
    
    if not model_dirs:
        print(f"Warning: No model directories found in {ocr_dir}")
        sys.exit(0)
    
    print(f"Found {len(model_dirs)} models")
    
    total = len(model_dirs)
    success_count = 0
    failed_models = []
    
    for model_dir in sorted(model_dirs):
        try:
            results = convert_model(model_dir, use_fp16)

            if args.install_dir and results['onnx_to_mnn']:
                results['install'] = install_converted_model(model_dir, args.install_dir)
            
            if any(results.values()):
                success_count += 1
            else:
                failed_models.append(model_dir.name)
        
        except Exception as e:
            print(f"  Error: {e}")
            failed_models.append(model_dir.name)
    
    print(f"\n{'='*60}")
    print(f"Completed: {success_count}/{total} successful")
    
    if failed_models:
        print(f"Failed: {len(failed_models)}")
        for model_name in failed_models:
            print(f"  - {model_name}")
    
    print()


if __name__ == '__main__':
    main()
