import { Component, Input, Output, EventEmitter, OnInit } from '@angular/core';
import { ViewCell } from 'ng2-smart-table';

@Component({
  selector: 'ngx-nested-link-render',
  template: `
    <a href="javascript:void(0)" (click)="onClick()" class="text-primary">{{ renderValue }}</a>
  `,
  styles: [`
    a {
      cursor: pointer;
    }
  `]
})
export class NestedLinkRenderComponent implements ViewCell, OnInit {
  @Input() value: string | number;
  @Input() rowData: any;
  @Output() save: EventEmitter<any> = new EventEmitter();
  
  renderValue: string;
  
  ngOnInit() {
    this.renderValue = String(this.value);
  }
  
  onClick() {
    this.save.emit(this.rowData);
  }
}
